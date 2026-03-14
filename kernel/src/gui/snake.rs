// kernel/src/gui/snake_game.rs

use alloc::vec::Vec;

const GRID_COLS: isize = 20;
const GRID_ROWS: isize = 15;

pub const TICKS_PER_UPDATE: usize = 3;

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
    pub last_tail:Option<Point>,
    pub moved:bool,
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
            last_tail:None,
            moved:false,
        };
        game.reset();
        game
    }

    pub fn reset(&mut self) {
        self.body.clear();
        let sx = GRID_COLS / 2;
        let sy = GRID_ROWS / 2;
        self.body.push(Point { x: sx, y: sy });
        self.body.push(Point { x: sx - 1, y: sy });
        self.body.push(Point { x: sx - 2, y: sy });
        self.direction = Direction::Right;
        self.next_direction = Direction::Right;
        self.state = GameState::Playing;
        self.score = 0;
        self.tick_counter = 0;
        self.moved = false;
        self.last_tail = None;
        // Vary the seed on reset so food isn't always the same
        self.rng_seed = self.rng_seed.wrapping_add(
            crate::interrupts::TICKS.load(core::sync::atomic::Ordering::Relaxed) as u32
        );
        self.spawn_food();
    }

    fn rand(&mut self) -> u32 {
        self.rng_seed = self.rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
        (self.rng_seed >> 16) & 0x7FFF
    }

    fn spawn_food(&mut self) {
        loop {
            let x = (self.rand() % GRID_COLS as u32) as isize;
            let y = (self.rand() % GRID_ROWS as u32) as isize;
            let pos = Point { x, y };
            if !self.body.contains(&pos) {
                self.food = pos;
                break;
            }
        }
    }

    pub fn on_key(&mut self, scancode: u8) {
        match self.state {
            GameState::Playing => {
                let new_dir = match scancode {
                    0x48 | 0x11 => Some(Direction::Up),
                    0x50 | 0x1F => Some(Direction::Down),
                    0x4B | 0x1E => Some(Direction::Left),
                    0x4D | 0x20 => Some(Direction::Right),
                    0x19 | 0x01 => { self.state = GameState::Paused; None }
                    _ => None,
                };
                if let Some(dir) = new_dir {
                    if !is_opposite(&dir, &self.direction) {
                        self.next_direction = dir;
                    }
                }
            }
            GameState::Paused => {
                if matches!(scancode, 0x19 | 0x01 | 0x1C) {
                    self.state = GameState::Playing;
                }
            }
            GameState::GameOver => {
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
        if self.tick_counter < TICKS_PER_UPDATE {
            return;
        }
        self.tick_counter = 0;
        self.update();
    }

    fn update(&mut self) {
        self.direction = self.next_direction;
        self.moved = false;

        let head = self.body[0];
        let new_head = match self.direction {
            Direction::Up    => Point { x: head.x,     y: head.y - 1 },
            Direction::Down  => Point { x: head.x,     y: head.y + 1 },
            Direction::Left  => Point { x: head.x - 1, y: head.y     },
            Direction::Right => Point { x: head.x + 1, y: head.y     },
        };

        if new_head.x < 0 || new_head.x >= GRID_COLS
            || new_head.y < 0 || new_head.y >= GRID_ROWS
        {
            self.game_over();
            return;
        }

        for seg in &self.body[..self.body.len() - 1] {
            if new_head == *seg {
                self.game_over();
                return;
            }
        }

        self.body.insert(0, new_head);

        if new_head == self.food {
            self.score += 10;
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