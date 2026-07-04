use alloc::collections::VecDeque;
use spin::Mutex;
use crate::process::{Process, ProcessState};
use core::sync::atomic::{AtomicU64, Ordering};

const MAX_LEVELS: usize = 4;
const BASE_QUANTUM_MS: [u32; MAX_LEVELS] = [2, 4, 8, 16];

const BASE_BOOST_INTERVAL_MS: u64 = 500;
const DEMOTION_THRESHOLD: [u64; MAX_LEVELS] = [10, 20, 40, u64::MAX]; // level 3 can't demote further
pub static SCHEDULER_TICKS: AtomicU64 = AtomicU64::new(0);
pub struct Scheduler {
    queues: [VecDeque<Process>; MAX_LEVELS],
    current: Option<Process>,
    last_boost_tick: u64,
    active_levels: usize,
    boost_interval: u64,
    pub total_switches: u64,
    last_known_load: usize,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            queues: [
                VecDeque::new(),
                VecDeque::new(),
                VecDeque::new(),
                VecDeque::new(),
            ],
            current: None,
            last_boost_tick: 0,
            active_levels: 2,       // start with 2 levels  for now 
            boost_interval: BASE_BOOST_INTERVAL_MS,
            total_switches: 0,
            last_known_load: 0,
        }
    }

    fn recalculate_dynamics(&mut self) {
        let load = self.process_count();
        if load == self.last_known_load {
            return; // no change
        }
        self.last_known_load = load;
        self.active_levels = if load <= 2 {
            2
        } else if load <= 5 {
            3
        } else {
            MAX_LEVELS // 4
        };
        self.boost_interval = if load <= 2 {
            BASE_BOOST_INTERVAL_MS * 2   // 1000ms run it on a easy mode.
        } else if load <= 5 {
            BASE_BOOST_INTERVAL_MS       // 500ms same here a bit for normal model
        } else if load <= 10 {
            BASE_BOOST_INTERVAL_MS / 2   // 250ms yeah push 
        } else {
            BASE_BOOST_INTERVAL_MS / 4   // 125ms full 
        };

        crate::serial_println!("[MLFQ] Load changed to {}. Active levels: {}, Boost interval: {}ms",load, self.active_levels, self.boost_interval);
    }

    //for now scaled by the load, so yeah 
    fn quantum_for(&self, level: usize) -> u32 {
        let clamped = level.min(self.active_levels - 1);
        let base = BASE_QUANTUM_MS[clamped];
        let load = self.process_count().max(1) as u32;

        if load <= 2 {
            base * 4              
        } else if load <= 5 {
            base                  
        } else if load <= 10 {
            (base / 2).max(1)     
        } else {
            (base / 4).max(1)     
        }
    }

    // we pritoize the work done on the  CPU threshold for a level, scaled by load.
    fn demotion_threshold_for(&self, level: usize) -> u64 {
        let clamped = level.min(MAX_LEVELS - 1);
        let base = DEMOTION_THRESHOLD[clamped];
        if base == u64::MAX {
            return u64::MAX; // bottom level, can't demote
        }
        let load = self.process_count().max(1) as u64;

        // Under heavy load, demote faster (less tolerance for CPU hogs)
        if load <= 2 {
            base * 3    // lenient — few processes, less contention
        } else if load <= 5 {
            base         // normal
        } else {
            (base / 2).max(1) // strict — many processes competing
        }
    }
    //so we here add  a new process, and initially hp.
    pub fn add_process(&mut self, mut process: Process) {
        process.state = ProcessState::Ready;
        process.priority = 0;
        process.cpu_since_boost = 0;
        self.queues[0].push_back(process);
        self.recalculate_dynamics();
    }
    //return true if an only if we see as context switch.
    pub fn timer_tick(&mut self) -> bool {
        let now = SCHEDULER_TICKS.load(Ordering::Relaxed);
        if now - self.last_boost_tick >= self.boost_interval {
            self.priority_boost();
            self.last_boost_tick = now;
        }
        if let Some(ref mut proc) = self.current {
            if proc.quantum_remaining > 0 {
                proc.quantum_remaining -= 1;
            }
            proc.cpu_since_boost += 1;
            if proc.quantum_remaining == 0 {
                return true;
            }
        }

        false
    }
    //running the next proc.
    pub fn schedule_next(&mut self) -> Option<(u64, *mut u64, u64)> {
        // Put the current process back into the appropriate queue
        if let Some(mut old_proc) = self.current.take() {
            if old_proc.state != ProcessState::Terminated {
                if old_proc.quantum_remaining == 0 {
                    self.check_and_demote(&mut old_proc);
                }
                old_proc.state = ProcessState::Ready;
                let level = (old_proc.priority as usize).min(self.active_levels - 1);
                self.queues[level].push_back(old_proc);
            }
        }
        self.pick_and_run()
    }
    pub fn yield_current(&mut self) -> Option<(u64, *mut u64, u64)> {
        if let Some(mut old_proc) = self.current.take() {
            if old_proc.state != ProcessState::Terminated {
                // Voluntary yield — check cumulative CPU for sneaky demotion
                self.check_cumulative_demotion(&mut old_proc);
                old_proc.state = ProcessState::Ready;
                let level = (old_proc.priority as usize).min(self.active_levels - 1);
                old_proc.quantum_remaining = self.quantum_for(level);
                self.queues[level].push_back(old_proc);
            }
        }

        self.pick_and_run()
    }
    //priotizing the process. as per the importance so yeah.. brrr more priority is push 
    fn pick_and_run(&mut self) -> Option<(u64, *mut u64, u64)> {
        // Scan only active levels
        let mut next_proc = None;
        for level in 0..self.active_levels {
            if let Some(proc) = self.queues[level].pop_front() {
                next_proc = Some(proc);
                break;
            }
        }
        if let Some(mut proc) = next_proc {
            let level = proc.priority as usize;
            proc.quantum_remaining = self.quantum_for(level);
            proc.state = ProcessState::Running;
            proc.total_ticks += 1;

            let new_stack = proc.stack_pointer.as_u64();
            let new_cr3 = proc.page_table.start_address().as_u64();

            self.current = Some(proc);
            self.total_switches += 1;

            let old_stack_ptr = if let Some(ref mut current) = self.current {
                &mut current.stack_pointer as *mut _ as *mut u64
            } else {
                return None;
            };

            Some((new_stack, old_stack_ptr, new_cr3))
        } else {
            None
        }
    }
    // if to be thrown out, call it here
    fn check_and_demote(&self, proc: &mut Process) {
        let level = proc.priority as usize;
        if level < self.active_levels - 1 {
            proc.priority += 1;
            crate::serial_println!("[MLFQ] Demoted '{}' (PID {}) → level {} (quantum exhausted, cpu_since_boost={})",proc.name, proc.id.0, proc.priority, proc.cpu_since_boost);
        }
    }
    fn check_cumulative_demotion(&self, proc: &mut Process) {
        let level = proc.priority as usize;
        let threshold = self.demotion_threshold_for(level);

        if proc.cpu_since_boost >= threshold && level < self.active_levels - 1 {
            proc.priority += 1;
            crate::serial_println!("[MLFQ] Demoted '{}' (PID {}) → level {} (cumulative CPU: {} >= threshold {})",proc.name, proc.id.0, proc.priority, proc.cpu_since_boost, threshold);
        }
    }

    // if a prc is in need high I/O we should give him the VIP spot
    pub fn promote_current(&mut self) {
        if let Some(ref mut proc) = self.current {
            if proc.priority > 0 {
                proc.priority -= 1;
                crate::serial_println!("[MLFQ] Promoted '{}' (PID {}) → level {}",proc.name, proc.id.0, proc.priority);
            }
        }
    }
    fn priority_boost(&mut self) {
        let mut boosted_count = 0;
        for level in 1..MAX_LEVELS {
            while let Some(mut proc) = self.queues[level].pop_front() {
                proc.priority = 0;
                proc.cpu_since_boost = 0;
                proc.quantum_remaining = self.quantum_for(0);
                self.queues[0].push_back(proc);
                boosted_count +=1;
            }
        }
        if let Some(ref mut proc) = self.current {
            if proc.priority > 0 {
                proc.priority = 0;
                proc.cpu_since_boost = 0;
            }
        }
        if boosted_count > 0 {
            crate::serial_println!("[MLFQ] Priority boost! Moved {} processes → level 0. Load: {}",boosted_count,self.process_count());
        }

        // Recalculate dynamics after boost (process states may have changed)
        self.recalculate_dynamics();
    }
    pub fn exit_current(&mut self, exit_code: u64) -> Option<(u64, u64)> {
        if let Some(proc) = self.current.take() {
            crate::serial_println!("[MLFQ] Process '{}' (PID {}) exited with code {}. Total CPU: {} ticks",proc.name, proc.id.0, exit_code, proc.total_ticks);
        }
        self.recalculate_dynamics();
        self.pick_and_run().map(|(new_stack, _, new_cr3)| (new_stack, new_cr3))
    }
    pub fn current_pid(&self) -> Option<u64> {
        self.current.as_ref().map(|p| p.id.0)
    }

    //Returns a mutable reference to the current process.
    pub fn current_process_mut(&mut self) -> Option<&mut Process> {
        self.current.as_mut()
    }
    // ahh this thing i am kind of doubtfull, this log kind of sucks, in the term,.. idts, this would work fine.
    pub fn print_process_list(&self, _active_pid: u64) {
        crate::println!("\nPID   | Name          | Level | Quantum | CPU/Boost | Status");
        crate::println!("------+---------------+-------+---------+-----------+--------");

        if let Some(ref proc) = self.current {
            crate::println!(
                "{:<5} | {:<13} | L{:<4} | {:<7} | {:<9} | Running",
                proc.id.0, proc.name, proc.priority,
                proc.quantum_remaining, proc.cpu_since_boost
            );
        }

        for level in 0..self.active_levels {
            for proc in &self.queues[level] {
                crate::println!(
                    "{:<5} | {:<13} | L{:<4} | {:<7} | {:<9} | Ready",
                    proc.id.0, proc.name, proc.priority,
                    proc.quantum_remaining, proc.cpu_since_boost
                );
            }
        }

        crate::println!("------+---------------+-------+---------+-----------+--------");
        crate::println!(
            "Load: {} | Active levels: {} | Boost interval: {}ms | Switches: {}",
            self.process_count(), self.active_levels,
            self.boost_interval, self.total_switches
        );
    }

    //Total number of processes (current + all queues) this should do 
    pub fn process_count(&self) -> usize {
        let queue_count: usize = self.queues.iter().map(|q| q.len()).sum();
        queue_count + if self.current.is_some() { 1 } else { 0 }
    }
    pub fn rotate_and_get_next(&mut self) -> Option<(u64, *mut u64, u64)> {
        self.yield_current()
    }
}

pub static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
const PIT_FREQUENCY: u32 = 1_193_182;
const TARGET_HZ: u32 = 1000;
const PIT_DIVISOR: u16 = (PIT_FREQUENCY / TARGET_HZ) as u16;
//from 18Hz to 1000hz, we are first running at 18Hz.
pub fn init_pit() {
    use x86_64::instructions::port::Port;
    unsafe {
        let mut cmd_port: Port<u8> = Port::new(0x43);
        cmd_port.write(0x34);

        let mut data_port: Port<u8> = Port::new(0x40);
        data_port.write((PIT_DIVISOR & 0xFF) as u8);
        data_port.write((PIT_DIVISOR >> 8) as u8);
    }
    crate::serial_println!("[MLFQ] PIT reprogrammed to {} Hz (divisor: {})",TARGET_HZ, PIT_DIVISOR);
}