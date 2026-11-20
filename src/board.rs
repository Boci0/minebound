use crate::{data::Element, rng::Rng};

pub const W: usize = 6;
pub const N: usize = W * W;
/// Crystals on a board: between these two, so no two boards feel alike.
pub const CRYSTALS: (usize, usize) = (8, 11);
/// Most solid rock a board can hold. Rock never cracks.
pub const MAX_ROCK: usize = 4;
/// A tile caves in when it reaches this many cracks.
pub const CAVE: u8 = 3;

#[derive(Clone, Copy, Default)]
pub struct Tile {
    /// gives a stack of the board's element to whoever mines it
    pub crystal: bool,
    /// solid rock: never cracks, never caves in
    pub rock: bool,
    /// 0 to `CAVE - 1`; at `CAVE` it caves in
    pub crack: u8,
    /// caved in: rubble for good, and a crystal there is lost
    pub caved: bool,
    /// a crystal already mined
    pub used: bool,
}

/// One board holds crystals of a single element, all in view. Mining one cracks the tiles around it.
#[derive(Clone)]
pub struct Board {
    pub element: Element,
    pub tiles: Vec<Tile>,
}

impl Board {
    pub fn deal(rng: &mut Rng, element: Element, weak: usize) -> Board {
        // every deal rolls its own mix: how many crystals, how much rock
        let crystals = CRYSTALS.0 + rng.below(CRYSTALS.1 - CRYSTALS.0 + 1);
        let rock = rng.below(MAX_ROCK + 1);
        let mut tiles: Vec<Tile> = (0..N).map(|i| Tile { crystal: i < crystals, rock: i >= crystals && i < crystals + rock, ..Tile::default() }).collect();
        rng.shuffle(&mut tiles);
        // a third of the ground starts hairline-cracked, and `weak` tiles are one crack from going
        let mut ground: Vec<usize> = (0..N).filter(|&i| !tiles[i].rock).collect();
        rng.shuffle(&mut ground);
        let weak = weak.min(ground.len());
        for (k, &i) in ground.iter().enumerate() {
            tiles[i].crack = if k < weak { CAVE - 1 } else if k < weak + ground.len() / 3 { 1 } else { 0 };
        }
        Board { element, tiles }
    }

    /// The tiles sharing an edge with `i`.
    pub fn around(i: usize) -> impl Iterator<Item = usize> {
        let (x, y) = (i % W, i / W);
        [(x > 0).then(|| i - 1), (x + 1 < W).then(|| i + 1), (y > 0).then(|| i - W), (y + 1 < W).then(|| i + W)].into_iter().flatten()
    }

    /// A crystal that can still be mined.
    pub fn live(&self, i: usize) -> bool {
        let t = self.tiles[i];
        t.crystal && !t.used && !t.caved
    }

    /// A tile that can still crack: not rock, not rubble.
    pub fn standing(&self, i: usize) -> bool {
        !self.tiles[i].rock && !self.tiles[i].caved
    }

    /// No crystal left to mine: time for a fresh board.
    pub fn spent(&self) -> bool {
        !(0..N).any(|i| self.live(i))
    }

    /// Adds a crack to each tile in `hit`. A tile that reaches `CAVE` caves in; it takes nothing else
    /// down with it, so what a pick can cost is always in view. Returns the tiles that caved in, in order.
    pub fn crack(&mut self, hit: impl IntoIterator<Item = usize>) -> Vec<usize> {
        let mut caved = vec![];
        for i in hit {
            if !self.standing(i) {
                continue;
            }
            self.tiles[i].crack += 1;
            if self.tiles[i].crack >= CAVE {
                self.tiles[i].caved = true;
                caved.push(i);
            }
        }
        caved
    }

    /// Mines crystal `i`: it is used up, and every tile touching it cracks. None if `i` is not a live
    /// crystal; otherwise the tiles that caved in.
    pub fn mine(&mut self, i: usize) -> Option<Vec<usize>> {
        if !self.live(i) {
            return None;
        }
        self.tiles[i].used = true;
        Some(self.crack(Board::around(i)))
    }

    /// Props tile `i` up: one crack fewer. False if it has none or cannot be propped.
    pub fn prop(&mut self, i: usize) -> bool {
        let ok = self.standing(i) && self.tiles[i].crack > 0;
        if ok {
            self.tiles[i].crack -= 1;
        }
        ok
    }

    /// The tiles a vent dug at `i` would ease: it and the tiles around it that hold a crack. Empty if
    /// no vent can go there (a live crystal, rock or rubble).
    pub fn vent_eases(&self, i: usize) -> Vec<usize> {
        if !self.standing(i) || self.live(i) {
            return vec![];
        }
        std::iter::once(i).chain(Board::around(i)).filter(|&j| self.standing(j) && self.tiles[j].crack > 0).collect()
    }

    /// Digs a vent at `i`: it and every tile around it lose a crack. False if it would ease nothing.
    pub fn vent(&mut self, i: usize) -> bool {
        let eased = self.vent_eases(i);
        for &j in &eased {
            self.tiles[j].crack -= 1;
        }
        !eased.is_empty()
    }

    /// Props up the `n` most cracked tiles, one crack each.
    pub fn ease(&mut self, n: usize) {
        let mut worst: Vec<usize> = (0..N).filter(|&i| self.standing(i) && self.tiles[i].crack > 0).collect();
        worst.sort_by_key(|&i| std::cmp::Reverse(self.tiles[i].crack));
        for &i in worst.iter().take(n) {
            self.prop(i);
        }
    }

    /// Cracks `n` standing tiles picked at random: an opponent's rite.
    /// Returns the tiles that caved in.
    pub fn quake(&mut self, n: usize, rng: &mut Rng) -> Vec<usize> {
        let mut spots: Vec<usize> = (0..N).filter(|&i| self.standing(i)).collect();
        rng.shuffle(&mut spots);
        spots.truncate(n);
        self.crack(spots)
    }

    /// The tiles mining crystal `i` would bring down, without touching the board.
    pub fn would_cave(&self, i: usize) -> Vec<usize> {
        self.clone().mine(i).unwrap_or_default()
    }

    /// Breaks up to `n` live crystals: they become plain tiles. Returns how many broke.
    pub fn shatter(&mut self, n: usize) -> usize {
        let live: Vec<usize> = (0..N).filter(|&i| self.live(i)).take(n).collect();
        for &i in &live {
            self.tiles[i].crystal = false;
        }
        live.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deal_has_crystals_rock_and_weak_tiles() {
        for seed in 0..50 {
            let b = Board::deal(&mut Rng::new(seed), Element::Ember, 4);
            assert_eq!(b.tiles.len(), N);
            let crystals = b.tiles.iter().filter(|t| t.crystal).count();
            assert!((CRYSTALS.0..=CRYSTALS.1).contains(&crystals));
            assert!(b.tiles.iter().filter(|t| t.rock).count() <= MAX_ROCK);
            assert!(b.tiles.iter().all(|t| !(t.crystal && t.rock) && t.crack < CAVE && !t.caved && !t.used));
            assert!(b.tiles.iter().all(|t| !t.rock || t.crack == 0), "rock never cracks");
            assert_eq!(b.tiles.iter().filter(|t| t.crack == CAVE - 1).count(), 4);
        }
        assert_eq!(Board::around(7).count() + Board::around(0).count(), 6);
        assert!(Board::around(W - 1).all(|j| j != W), "no wrapping round the edge");
    }

    /// A blank board with crystals at `gems` and the given cracks.
    fn blank(gems: &[usize], cracks: &[(usize, u8)]) -> Board {
        let mut b = Board { element: Element::Ember, tiles: vec![Tile::default(); N] };
        for &i in gems {
            b.tiles[i].crystal = true;
        }
        for &(i, c) in cracks {
            b.tiles[i].crack = c;
        }
        b
    }

    #[test]
    fn mining_cracks_the_neighbours_and_a_cave_in_stays_put() {
        // 7 touches 1, 6, 8 and 13; 8 is about to cave in, and 9 beside it too
        let mut b = blank(&[7], &[(8, 2), (9, 2)]);
        assert_eq!(b.would_cave(7), vec![8], "8 falls, and takes nothing with it");
        assert!(b.live(7), "looking ahead changes nothing");
        assert_eq!(b.mine(7), Some(vec![8]));
        assert!(!b.live(7) && b.mine(7).is_none(), "a crystal is mined once");
        assert_eq!((b.tiles[1].crack, b.tiles[6].crack, b.tiles[13].crack, b.tiles[9].crack), (1, 1, 1, 2));
        assert!(b.tiles[8].caved && !b.prop(8), "rubble stays rubble");

        // a crystal that caves in is lost, and rock never cracks
        let mut b = blank(&[0, 1, 2], &[(1, 2)]);
        b.tiles[6].rock = true;
        assert_eq!(b.mine(0), Some(vec![1]));
        assert!(!b.live(1) && b.live(2));
        assert_eq!(b.tiles[6].crack, 0);
    }

    #[test]
    fn props_ease_quake_and_shatter() {
        let mut b = blank(&[0, 1, 2], &[(5, 2), (11, 1)]);
        assert!(b.prop(5) && b.tiles[5].crack == 1 && !b.prop(4), "an uncracked tile needs no prop");
        b.ease(2);
        assert_eq!((b.tiles[5].crack, b.tiles[11].crack), (0, 0));
        let mut rng = Rng::new(3);
        for _ in 0..CAVE {
            b.quake(N, &mut rng);
        }
        assert!(b.tiles.iter().all(|t| t.caved) && b.spent(), "crack every tile enough and the board comes down");
        let mut b = blank(&[0, 1, 2], &[]);
        assert_eq!(b.shatter(2), 2);

        // a vent eases itself and the tiles around it, as often as there is a crack to ease, never on a live crystal
        let mut b = blank(&[7], &[(8, 2), (9, 1), (2, 2), (14, 1), (6, 1)]);
        assert!(b.vent_eases(7).is_empty() && b.vent_eases(30).is_empty(), "not on a crystal, and not with nothing to ease");
        assert_eq!(b.vent_eases(8), vec![8, 9, 2, 14]);
        assert!(b.vent(8));
        assert_eq!((b.tiles[8].crack, b.tiles[9].crack, b.tiles[2].crack, b.tiles[14].crack, b.tiles[6].crack), (1, 0, 1, 0, 1));
        assert!(b.vent(8) && b.tiles[8].crack == 0 && b.tiles[2].crack == 0, "the same tile vents again");
        assert!(!b.vent(8), "but not with nothing left to ease");
        assert_eq!((0..N).filter(|&i| b.live(i)).count(), 1);
    }
}
