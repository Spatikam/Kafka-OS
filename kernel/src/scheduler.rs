use crate::process::Process;
use alloc::collections::VecDeque;
use spin::Mutex;
use x86_64::structures::paging::PhysFrame;

pub struct Scheduler {
    processes: VecDeque<Process>,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            processes: VecDeque::new(),
        }
    }

    pub fn add_process(&mut self, process: Process) {
        self.processes.push_back(process);
    }

    // Returns (new_stack_ptr, old_stack_save_location, new_cr3_value)
    // Changed PhysFrame to u64 for new_cr3 so we can pass it directly to assembly
    pub fn rotate_and_get_next(&mut self) -> Option<(u64, *mut u64, u64)> {
        if self.processes.is_empty() {
            return None;
        }
        if let Some(p) = self.processes.pop_front() {
            self.processes.push_back(p);
        }
        let next_process = self.processes.front()?;
        let new_stack = next_process.stack_pointer.as_u64();
        // Extract the physical address as u64 so we can write directly to CR3
        let new_cr3 = next_process.page_table.start_address().as_u64();
        let old_process = self.processes.back_mut()?;
        let old_stack_ptr_location = &mut old_process.stack_pointer as *mut _ as *mut u64;
        Some((new_stack, old_stack_ptr_location, new_cr3))
    }

    pub fn print_process_list(&self, active_pid: u64) {
        crate::println!("\nPID   | Name          | Status");
        crate::println!("------+---------------+--------");
        for process in &self.processes {
            let state_label = if process.id.0 == active_pid {
                "Running"
            } else {
                "Ready"
            };
            crate::println!(
                "{:<5} | {:<13} | {}",
                process.id.0,
                process.name,
                state_label
            );
        }
        crate::println!("------+---------------+--------");
    }

    pub fn process_count(&self) -> usize {
        self.processes.len()
    }
}

pub static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());
