use crate::{data::Element, rng::Rng};

pub const W: usize = 6;
pub const N: usize = W * W;
/// Crystals on a board: between these two, so no two boards feel alike.
pub const CRYSTALS: (usize, usize) = (4, 6);
/// Most solid rock a board can hold. Rock cannot be flipped and needs no clearing.
pub const MAX_ROCK: usize = 6;

#[derive(Clone, Copy, Default)]
pub struct Tile {
    pub mine: bool,
    /// always visible, never clickable; clear every safe tile around it to claim a stack
    pub crystal: bool,
    /// solid rock: never flipped, hides nothing, shapes the board
    pub rock: bool,
    /// crystal only: its stack has been collected
    pub claimed: bool,
    /// crystal only: a mine went off next to it, so it gives nothing
    pub broken: bool,
    pub revealed: bool,
    pub flagged: bool,
}

/// One board holds crystals of a single element.
#[derive(Clone)]
pub struct Board {
    pub element: Element,
    pub tiles: Vec<Tile>,
    pub mines: usize,
    /// nothing flipped yet: the first flip is guaranteed safe
    pub fresh: bool,
}

/// What flipping a tile did.
pub struct Flip {
    pub mine: bool,
    /// crystals whose surroundings are now fully cleared
    pub claimed: usize,
    /// crystals a mine just broke
    pub broken: usize,
}

impl Board {
    pub fn deal(rng: &mut Rng, element: Element, mines: usize) -> Board {
        // every deal rolls its own mix: how many crystals, how much rock
        let crystals = CRYSTALS.0 + rng.below(CRYSTALS.1 - CRYSTALS.0 + 1);
        let rock = rng.below(MAX_ROCK + 1);
        let mut tiles: Vec<Tile> = (0..N)
            .map(|i| Tile {
                mine: i < mines,
                crystal: i >= mines && i < mines + crystals,
                rock: i >= mines + crystals && i < mines + crystals + rock,
                ..Tile::default()
            })
            .collect();
        rng.shuffle(&mut tiles);
        Board { element, tiles, mines, fresh: true }
    }

    pub fn neighbors(i: usize) -> impl Iterator<Item = usize> {
        let (x, y) = ((i % W) as i32, (i / W) as i32);
        (-1..=1)
            .flat_map(move |dy| (-1..=1).map(move |dx| (x + dx, y + dy)))
            .filter(move |&(nx, ny)| {
                (nx, ny) != (x, y) && nx >= 0 && ny >= 0 && nx < W as i32 && ny < W as i32
            })
            .map(|(nx, ny)| ny as usize * W + nx as usize)
    }

    /// The minesweeper hint: mines touching tile `i`.
    pub fn adjacent(&self, i: usize) -> usize {
        Self::neighbors(i).filter(|&j| self.tiles[j].mine).count()
    }

    /// A crystal that can still be claimed.
    pub fn live(&self, i: usize) -> bool {
        let t = &self.tiles[i];
        t.crystal && !t.claimed && !t.broken
    }

    /// Hidden tiles still standing between crystal `c` and its stack (mines included: you cannot tell).
    pub fn left_around(&self, c: usize) -> usize {
        Self::neighbors(c).filter(|&j| !self.tiles[j].revealed && !self.tiles[j].crystal && !self.tiles[j].rock).count()
    }

    /// No crystal left to claim: time for a fresh board.
    pub fn spent(&self) -> bool {
        !(0..N).any(|i| self.live(i))
    }

    /// Hidden, unflagged and not a crystal: a tile that can be flipped.
    pub fn open(&self, i: usize) -> bool {
        let t = &self.tiles[i];
        !t.revealed && !t.flagged && !t.crystal && !t.rock
    }

    /// Safe opening: if the first tile flipped on a board hides a mine, the mine moves elsewhere.
    /// The first click is never a lost turn, but it is not a free area either.
    fn clear_opening(&mut self, i: usize, rng: &mut Rng) {
        let mut spare: Vec<usize> = (0..N).filter(|&k| k != i && !self.tiles[k].mine && !self.tiles[k].crystal && !self.tiles[k].rock).collect();
        rng.shuffle(&mut spare);
        if let (true, Some(k)) = (self.tiles[i].mine, spare.pop()) {
            // a scouting flag travels with its mine
            self.tiles[k].mine = true;
            self.tiles[k].flagged = self.tiles[i].flagged;
            self.tiles[i].mine = false;
            self.tiles[i].flagged = false;
        }
    }

    /// Flip open tile `i`. A safe "0" opens its neighbours too, like minesweeper.
    /// A mine breaks every unclaimed crystal it touches.
    pub fn flip(&mut self, i: usize, rng: &mut Rng) -> Flip {
        if std::mem::take(&mut self.fresh) {
            self.clear_opening(i, rng);
        }
        self.tiles[i].revealed = true;
        if self.tiles[i].mine {
            let mut broken = 0;
            for j in Self::neighbors(i) {
                if self.live(j) {
                    self.tiles[j].broken = true;
                    broken += 1;
                }
            }
            return Flip { mine: true, claimed: 0, broken };
        }
        let mut todo = vec![i];
        while let Some(k) = todo.pop() {
            if self.adjacent(k) == 0 {
                for j in Self::neighbors(k) {
                    if self.open(j) {
                        self.tiles[j].revealed = true;
                        todo.push(j);
                    }
                }
            }
        }
        let mut claimed = 0;
        for c in 0..N {
            // "cleared" means every safe neighbour is revealed; mines stay hidden
            if self.live(c) && Self::neighbors(c).all(|j| self.tiles[j].revealed || self.tiles[j].mine || self.tiles[j].crystal || self.tiles[j].rock) {
                self.tiles[c].claimed = true;
                claimed += 1;
            }
        }
        Flip { mine: false, claimed, broken: 0 }
    }

    fn known_mine(&self, i: usize) -> bool {
        self.tiles[i].flagged || (self.tiles[i].revealed && self.tiles[i].mine)
    }

    /// Flags up to `n` hidden mines (scouting effects).
    pub fn flag_mines(&mut self, n: usize) {
        for t in self.tiles.iter_mut().filter(|t| t.mine && !t.revealed && !t.flagged).take(n) {
            t.flagged = true;
        }
    }

    /// Chance that open tile `i` is a mine, judged only from what is visible (the AI does not cheat).
    pub fn risk(&self, i: usize) -> f32 {
        if self.fresh {
            return 0.0; // safe opening
        }
        let mut risk: Option<f32> = None;
        for j in Self::neighbors(i).filter(|&j| self.tiles[j].revealed && !self.tiles[j].mine) {
            let known = Self::neighbors(j).filter(|&k| self.known_mine(k)).count();
            let open = Self::neighbors(j).filter(|&k| self.open(k)).count();
            let local = self.adjacent(j).saturating_sub(known) as f32 / open.max(1) as f32;
            // one hint proving the tile safe beats any other hint
            risk = Some(match risk {
                Some(r) if r == 0.0 || local == 0.0 => 0.0,
                Some(r) => r.max(local),
                None => local,
            });
        }
        risk.unwrap_or_else(|| {
            let known = (0..N).filter(|&k| self.known_mine(k)).count();
            let open = (0..N).filter(|&k| self.open(k)).count();
            self.mines.saturating_sub(known) as f32 / open.max(1) as f32
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deal_has_exact_mines_crystals_and_true_hints() {
        let mut rng = Rng::new(7);
        let b = Board::deal(&mut rng, Element::Ember, 6);
        assert_eq!(b.tiles.len(), N);
        assert_eq!(b.tiles.iter().filter(|t| t.mine).count(), 6);
        let crystals = b.tiles.iter().filter(|t| t.crystal).count();
        assert!((CRYSTALS.0..=CRYSTALS.1).contains(&crystals));
        assert!(b.tiles.iter().filter(|t| t.rock).count() <= MAX_ROCK);
        assert!(b.tiles.iter().all(|t| t.mine as u8 + t.crystal as u8 + t.rock as u8 <= 1));
        assert_eq!(Board::neighbors(0).count(), 3);
        assert_eq!(Board::neighbors(7).count(), 8);
        let around_mines: usize = (0..N).filter(|&i| b.tiles[i].mine).map(|i| Board::neighbors(i).count()).sum();
        assert_eq!((0..N).map(|i| b.adjacent(i)).sum::<usize>(), around_mines);
    }

    #[test]
    fn a_crystal_is_claimed_only_when_every_safe_tile_around_it_is_clear() {
        for seed in 0..50 {
            let mut b = Board::deal(&mut Rng::new(seed), Element::Ember, 6);
            b.fresh = false;
            let c = (0..N).find(|&i| b.tiles[i].crystal).unwrap();
            let safe: Vec<usize> = Board::neighbors(c).filter(|&j| !b.tiles[j].mine && !b.tiles[j].crystal && !b.tiles[j].rock).collect();
            let mut claimed_at = None;
            for (step, &j) in safe.iter().enumerate() {
                if b.open(j) {
                    b.flip(j, &mut Rng::new(1));
                }
                if b.tiles[c].claimed && claimed_at.is_none() {
                    claimed_at = Some(step);
                }
                let all_clear = safe.iter().all(|&k| b.tiles[k].revealed);
                assert_eq!(b.tiles[c].claimed, all_clear || safe.is_empty(), "seed {seed}");
            }
            assert!(b.tiles.iter().all(|t| !(t.mine && t.revealed)), "clearing never opens a mine");
        }
    }

    #[test]
    fn the_first_flip_on_a_board_is_never_a_mine() {
        for seed in 0..100 {
            let mut rng = Rng::new(seed);
            let mut b = Board::deal(&mut rng, Element::Ember, 8);
            b.flag_mines(2);
            let i = (0..N).filter(|&i| b.open(i)).nth(seed as usize % 20).unwrap();
            assert!(!b.flip(i, &mut rng).mine, "seed {seed}");
            assert_eq!(b.tiles.iter().filter(|t| t.mine).count(), 8, "mines move, none vanish");
            assert!(b.tiles.iter().all(|t| !t.flagged || t.mine), "flags still mark mines");
        }
    }

    #[test]
    fn a_mine_breaks_the_crystals_it_touches() {
        for seed in 0..200 {
            let mut b = Board::deal(&mut Rng::new(seed), Element::Ember, 6);
            b.fresh = false;
            let Some(m) = (0..N).find(|&i| b.tiles[i].mine && Board::neighbors(i).any(|j| b.tiles[j].crystal)) else { continue };
            assert!(b.flip(m, &mut Rng::new(1)).mine);
            for j in Board::neighbors(m).filter(|&j| b.tiles[j].crystal) {
                assert!(b.tiles[j].broken && !b.live(j));
            }
            return;
        }
        panic!("no seed put a mine next to a crystal");
    }

    #[test]
    fn a_proven_mine_reads_as_certain_and_crystals_are_never_open() {
        let mut rng = Rng::new(3);
        let mut b = Board::deal(&mut rng, Element::Ember, 6);
        b.fresh = false;
        for t in b.tiles.iter_mut().filter(|t| !t.mine && !t.crystal && !t.rock) {
            t.revealed = true;
        }
        for i in 0..N {
            assert_eq!(b.open(i), b.tiles[i].mine);
            if b.open(i) {
                assert!(b.risk(i) >= 1.0);
            }
        }
    }
}
