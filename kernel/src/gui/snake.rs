// kernel/src/gui/snake.rs

use alloc::vec::Vec;

const GRID_COLS: isize = 20;
const GRID_ROWS: isize = 15;

/// Base ticks between updates (decreases as score climbs)
const BASE_TICKS: usize = 2;
/// Fastest the game can go
const MIN_TICKS: usize = 1;
/// Score interval at which speed increases
const SPEED_STEP: u32 = 60;

#[derive(Clone, Copy, PartialEq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Point {
    pub x: isize,
    pub y: isize,
}

#[derive(Clone, Copy, PartialEq)]
pub enum GameState {
    Playing,
    GameOver,
    Paused,
}

pub struct SnakeGame {
    pub body: Vec<Point>,
    pub direction: Direction,
    pub next_direction: Direction,
    pub food: Point,
    pub state: GameState,
    pub score: u32,
    pub high_score: u32,
    pub tick_counter: usize,
    rng_seed: u32,
    pub last_tail: Option<Point>,
    pub moved: bool,
    /// Set for one frame when food is eaten (lets renderer flash)
    pub just_ate: bool,
    pub grid_cols: isize,
    pub grid_rows: isize,
}

impl SnakeGame {
    pub fn new() -> Self {
        let mut game = SnakeGame {
            body: Vec::new(),
            direction: Direction::Right,
            next_direction: Direction::Right,
            food: Point { x: 0, y: 0 },
            state: GameState::Playing,
            score: 0,
            high_score: 0,
            tick_counter: 0,
            rng_seed: 54321,
            last_tail: None,
            moved: false,
            just_ate: false,
            grid_cols: GRID_COLS,
            grid_rows: GRID_ROWS,
        };
        game.reset();
        game
    }

    pub fn reset(&mut self) {
        self.body.clear();
        let sx = GRID_COLS / 2;
        let sy = GRID_ROWS / 2;
        // Start with a 4-segment snake for better visuals
        self.body.push(Point { x: sx, y: sy });
        self.body.push(Point { x: sx - 1, y: sy });
        self.body.push(Point { x: sx - 2, y: sy });
        self.body.push(Point { x: sx - 3, y: sy });
        self.direction = Direction::Right;
        self.next_direction = Direction::Right;
        self.state = GameState::Playing;
        self.score = 0;
        self.tick_counter = 0;
        self.moved = false;
        self.just_ate = false;
        self.last_tail = None;
        // Vary the seed on reset so food isn't always the same
        self.rng_seed = self.rng_seed.wrapping_add(
            crate::interrupts::TICKS.load(core::sync::atomic::Ordering::Relaxed) as u32,
        );
        self.spawn_food();
    }

    /// Current ticks-per-update — gets faster as score climbs
    pub fn ticks_per_update(&self) -> usize {
        let levels = (self.score / SPEED_STEP) as usize;
        BASE_TICKS.saturating_sub(levels).max(MIN_TICKS)
    }

    fn rand(&mut self) -> u32 {
        // xorshift32 — better distribution than LCG for small grids
        let mut s = self.rng_seed;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.rng_seed = s;
        s
    }

    fn spawn_food(&mut self) {
        // Build a list of free cells so we never spin on a nearly-full board
        let mut free: Vec<Point> = Vec::new();
        for y in 0..GRID_ROWS {
            for x in 0..GRID_COLS {
                let p = Point { x, y };
                if !self.body.contains(&p) {
                    free.push(p);
                }
            }
        }
        if free.is_empty() {
            // Player filled the entire board — they win! Treat as game over.
            self.game_over();
            return;
        }
        let idx = (self.rand() as usize) % free.len();
        self.food = free[idx];
    }

    pub fn on_key(&mut self, scancode: u8) {
        match self.state {
            GameState::Playing => {
                let new_dir = match scancode {
                    0x48 | 0x11 => Some(Direction::Up),    // Arrow Up / W
                    0x50 | 0x1F => Some(Direction::Down),  // Arrow Down / S
                    0x4B | 0x1E => Some(Direction::Left),  // Arrow Left / A
                    0x4D | 0x20 => Some(Direction::Right), // Arrow Right / D
                    0x19 | 0x01 => {
                        // P or Escape → pause
                        self.state = GameState::Paused;
                        None
                    }
                    _ => None,
                };
                if let Some(dir) = new_dir {
                    if !is_opposite(&dir, &self.direction) {
                        self.next_direction = dir;
                    }
                }
            }
            GameState::Paused => {
                // Any of P / Escape / Enter unpauses
                if matches!(scancode, 0x19 | 0x01 | 0x1C) {
                    self.state = GameState::Playing;
                }
            }
            GameState::GameOver => {
                // Enter or Space restarts
                if matches!(scancode, 0x1C | 0x39) {
                    self.reset();
                }
            }
        }
    }

    pub fn tick(&mut self) {
        if self.state != GameState::Playing {
            return;
        }
        self.tick_counter += 1;
        if self.tick_counter < self.ticks_per_update() {
            return;
        }
        self.tick_counter = 0;
        self.update();
    }

    fn update(&mut self) {
        self.direction = self.next_direction;
        self.moved = false;
        self.just_ate = false;

        let head = self.body[0];
        let new_head = match self.direction {
            Direction::Up => Point { x: head.x, y: head.y - 1 },
            Direction::Down => Point { x: head.x, y: head.y + 1 },
            Direction::Left => Point { x: head.x - 1, y: head.y },
            Direction::Right => Point { x: head.x + 1, y: head.y },
        };

        // Wall collision
        if new_head.x < 0
            || new_head.x >= GRID_COLS
            || new_head.y < 0
            || new_head.y >= GRID_ROWS
        {
            self.game_over();
            return;
        }

        // Self collision (exclude tail — it will move out of the way)
        for seg in &self.body[..self.body.len() - 1] {
            if new_head == *seg {
                self.game_over();
                return;
            }
        }

        self.body.insert(0, new_head);

        if new_head == self.food {
            self.score += 10;
            self.just_ate = true;
            self.last_tail = None;
            self.spawn_food();
        } else {
            self.last_tail = self.body.pop();
        }
        self.moved = true;
    }

    fn game_over(&mut self) {
        self.state = GameState::GameOver;
        if self.score > self.high_score {
            self.high_score = self.score;
        }
    }

    /// Returns a color gradient index (0–255) for body segment `i` out of `len`
    pub fn body_gradient(&self, i: usize) -> u8 {
        let len = self.body.len().max(1);
        // Head = bright (200), tail = dim (60)
        let ratio = i as u32 * 140 / len as u32;
        (200u32.saturating_sub(ratio)) as u8
    }
}

fn is_opposite(a: &Direction, b: &Direction) -> bool {
    matches!(
        (a, b),
        (Direction::Up, Direction::Down)
            | (Direction::Down, Direction::Up)
            | (Direction::Left, Direction::Right)
            | (Direction::Right, Direction::Left)
    )
}