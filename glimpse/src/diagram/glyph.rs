//! Box-drawing glyph resolution for cells where edges meet.

/// Which sides of a cell an edge touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ArmMask(pub u8);

impl ArmMask {
    pub(crate) const NONE: ArmMask = ArmMask(0);
    pub(crate) const N: ArmMask = ArmMask(1);
    pub(crate) const E: ArmMask = ArmMask(2);
    pub(crate) const S: ArmMask = ArmMask(4);
    pub(crate) const W: ArmMask = ArmMask(8);

    pub(crate) fn union(self, other: ArmMask) -> ArmMask {
        ArmMask(self.0 | other.0)
    }

    pub(crate) fn has(self, other: ArmMask) -> bool {
        self.0 & other.0 != 0
    }

    fn has_vertical(self) -> bool {
        self.has(ArmMask::N.union(ArmMask::S))
    }
}

/// One edge's claim on a cell: its source group, the arms it uses and its line style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Owner {
    pub edge_group: u32,
    pub arms: ArmMask,
    pub dashed: bool,
}

/// Up to four edges sharing one cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CellOwners {
    owners: [Option<Owner>; 4],
}

impl CellOwners {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Records an owner; returns false when all four slots are taken.
    pub(crate) fn push(&mut self, edge_group: u32, arms: ArmMask, dashed: bool) -> bool {
        match self.owners.iter_mut().find(|slot| slot.is_none()) {
            Some(slot) => {
                *slot = Some(Owner {
                    edge_group,
                    arms,
                    dashed,
                });
                true
            }
            None => false,
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = Owner> + '_ {
        self.owners.iter().flatten().copied()
    }
}

/// Resolves the character for a cell.
///
/// Owners of one group merge their arms. Where several groups meet, owners with a
/// vertical arm win and the rest are dropped, so the vertical stays unbroken and
/// the horizontal is interrupted. The dashed glyphs apply only to a straight
/// segment whose surviving owners are all dashed.
pub(crate) fn glyph(cell: &CellOwners, rounded: bool) -> char {
    let owners: Vec<Owner> = cell.iter().collect();
    let first_group = match owners.first() {
        Some(o) => o.edge_group,
        None => return ' ',
    };
    let mixed = owners.iter().any(|o| o.edge_group != first_group);
    let kept: Vec<Owner> = if mixed && owners.iter().any(|o| o.arms.has_vertical()) {
        owners
            .into_iter()
            .filter(|o| o.arms.has_vertical())
            .collect()
    } else {
        owners
    };
    let arms = kept.iter().fold(ArmMask::NONE, |m, o| m.union(o.arms));
    let dashed = kept.iter().all(|o| o.dashed);
    from_arms(arms, rounded, dashed)
}

fn from_arms(arms: ArmMask, rounded: bool, dashed: bool) -> char {
    let (n, e, s, w) = (
        arms.has(ArmMask::N),
        arms.has(ArmMask::E),
        arms.has(ArmMask::S),
        arms.has(ArmMask::W),
    );
    match (n, e, s, w) {
        (false, false, false, false) => ' ',
        (true, false, false, false) | (false, false, true, false) | (true, false, true, false) => {
            if dashed {
                '┆'
            } else {
                '│'
            }
        }
        (false, true, false, false) | (false, false, false, true) | (false, true, false, true) => {
            if dashed {
                '┄'
            } else {
                '─'
            }
        }
        (true, true, false, false) => {
            if rounded {
                '╰'
            } else {
                '└'
            }
        }
        (true, false, false, true) => {
            if rounded {
                '╯'
            } else {
                '┘'
            }
        }
        (false, true, true, false) => {
            if rounded {
                '╭'
            } else {
                '┌'
            }
        }
        (false, false, true, true) => {
            if rounded {
                '╮'
            } else {
                '┐'
            }
        }
        (true, true, true, false) => '├',
        (true, false, true, true) => '┤',
        (false, true, true, true) => '┬',
        (true, true, false, true) => '┴',
        (true, true, true, true) => '┼',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single(mask: u8, dashed: bool) -> CellOwners {
        let mut c = CellOwners::new();
        c.push(1, ArmMask(mask), dashed);
        c
    }

    #[test]
    fn all_sixteen_masks_square_and_rounded() {
        let square = [
            ' ', '│', '─', '└', '│', '│', '┌', '├', '─', '┘', '─', '┴', '┐', '┤', '┬', '┼',
        ];
        let rounded = [
            ' ', '│', '─', '╰', '│', '│', '╭', '├', '─', '╯', '─', '┴', '╮', '┤', '┬', '┼',
        ];
        for mask in 0u8..16 {
            assert_eq!(
                glyph(&single(mask, false), false),
                square[mask as usize],
                "square {mask}"
            );
            assert_eq!(
                glyph(&single(mask, false), true),
                rounded[mask as usize],
                "rounded {mask}"
            );
        }
    }

    #[test]
    fn same_group_owners_merge_arms() {
        let mut c = CellOwners::new();
        c.push(7, ArmMask::N.union(ArmMask::S), false);
        c.push(7, ArmMask::E, false);
        assert_eq!(glyph(&c, false), '├');
    }

    #[test]
    fn unrelated_crossing_keeps_vertical_and_breaks_horizontal() {
        let mut c = CellOwners::new();
        c.push(1, ArmMask::E.union(ArmMask::W), false);
        c.push(2, ArmMask::N.union(ArmMask::S), false);
        assert_eq!(glyph(&c, false), '│');
    }

    #[test]
    fn dashed_straight_segments() {
        assert_eq!(
            glyph(&single(ArmMask::N.union(ArmMask::S).0, true), false),
            '┆'
        );
        assert_eq!(
            glyph(&single(ArmMask::E.union(ArmMask::W).0, true), false),
            '┄'
        );
        assert_eq!(
            glyph(&single(ArmMask::N.union(ArmMask::E).0, true), false),
            '└'
        );
    }
}
