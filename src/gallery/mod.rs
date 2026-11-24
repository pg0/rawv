//! Folder view: the open folder and its subfolders shown as rolls of film.

pub mod layout;
mod view;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::command::Direction;
use crate::library::lister::{ListResult, Lister};
use crate::library::listing::{self, ImageEntry, SortOrder};

pub use view::GalleryContext;

pub enum RollState {
    Unrequested,
    Loading,
    Ready {
        images: Arc<[ImageEntry]>,
        folder_count: usize,
        /// Date of the oldest image, for the roll's caption.
        date: Option<String>,
    },
    Failed(String),
}

pub struct Roll {
    pub path: PathBuf,
    pub title: String,
    /// Names of the folders between the open folder and this one, outermost first.
    pub trail: Vec<String>,
    /// The open folder itself, as opposed to one of its subfolders.
    pub is_root: bool,
    pub expanded: bool,
    pub state: RollState,
}

impl Roll {
    pub fn images(&self) -> &[ImageEntry] {
        match &self.state {
            RollState::Ready { images, .. } => images,
            _ => &[],
        }
    }

    pub fn folder_count(&self) -> usize {
        match &self.state {
            RollState::Ready { folder_count, .. } => *folder_count,
            _ => 0,
        }
    }
}

/// What a finished listing changed.
pub struct Listed {
    pub roll: usize,
    /// Rolls added directly after `roll`; later rolls moved down by this many.
    pub inserted: usize,
}

#[derive(Default)]
pub struct Gallery {
    pub folder: Option<PathBuf>,
    pub rolls: Vec<Roll>,
    /// Roll index and image index.
    pub selection: Option<(usize, usize)>,
    pub sort: SortOrder,
    pub thumb_scale: f32,
    /// Every nested subfolder gets a roll, not only the direct ones.
    pub nested: bool,
    generation: u64,
    reveal_selection: bool,
    /// Frames per row in the last frame drawn, for keyboard navigation.
    columns: usize,
}

impl Gallery {
    pub fn new(sort: SortOrder, thumb_scale: f32, nested: bool) -> Self {
        Self {
            sort,
            thumb_scale,
            nested,
            columns: 1,
            ..Self::default()
        }
    }

    pub fn open(&mut self, folder: &Path, lister: &mut Lister) {
        self.generation += 1;
        self.selection = None;
        self.folder = Some(folder.to_path_buf());
        self.rolls = vec![Roll {
            path: folder.to_path_buf(),
            title: folder_title(folder),
            trail: Vec::new(),
            is_root: true,
            expanded: true,
            state: RollState::Loading,
        }];
        lister.request(self.generation, folder.to_path_buf());
    }

    pub fn refresh(&mut self, lister: &mut Lister) {
        if let Some(folder) = self.folder.clone() {
            self.open(&folder, lister);
        }
    }

    pub fn set_nested(&mut self, nested: bool, lister: &mut Lister) {
        self.nested = nested;
        self.refresh(lister);
    }

    /// Applies a finished listing and adds rolls for the subfolders it found.
    pub fn on_listing(&mut self, result: ListResult, lister: &mut Lister) -> Option<Listed> {
        if result.ticket != self.generation {
            return None;
        }
        let index = self
            .rolls
            .iter()
            .position(|roll| roll.path == result.path)?;
        let listing = match result.listing {
            Ok(listing) => listing,
            Err(message) => {
                self.rolls[index].state = RollState::Failed(message);
                return Some(Listed {
                    roll: index,
                    inserted: 0,
                });
            }
        };
        let mut images = listing.images;
        listing::sort_images(&mut images, self.sort);
        let date = images
            .iter()
            .map(|image| image.modified)
            .min()
            .map(|oldest| {
                chrono::DateTime::<chrono::Local>::from(oldest)
                    .format("%b %e '%y")
                    .to_string()
            });
        let parent = &self.rolls[index];
        let children: Vec<Roll> = if parent.is_root || self.nested {
            let trail: Vec<String> = if parent.is_root {
                Vec::new()
            } else {
                parent
                    .trail
                    .iter()
                    .chain([&parent.title])
                    .cloned()
                    .collect()
            };
            let nested = self.nested;
            let folders = listing.folders.iter();
            folders
                .map(|folder| Roll {
                    path: folder.path.clone(),
                    title: folder.name.clone(),
                    trail: trail.clone(),
                    is_root: false,
                    expanded: false,
                    state: if nested {
                        RollState::Loading
                    } else {
                        RollState::Unrequested
                    },
                })
                .collect()
        } else {
            Vec::new()
        };
        let inserted = children.len();
        if self.nested {
            for child in &children {
                lister.request(self.generation, child.path.clone());
            }
        }
        self.rolls.splice(index + 1..index + 1, children);
        if let Some((roll, _)) = &mut self.selection
            && *roll > index
        {
            *roll += inserted;
        }
        self.rolls[index].state = RollState::Ready {
            images: Arc::from(images),
            folder_count: listing.folders.len(),
            date,
        };
        Some(Listed {
            roll: index,
            inserted,
        })
    }

    /// The nearest roll with images before (`direction < 0`) or after `roll`.
    pub fn neighbour_roll(&self, roll: usize, direction: isize) -> Option<usize> {
        let has_images = |&index: &usize| !self.rolls[index].images().is_empty();
        if direction < 0 {
            (0..roll.min(self.rolls.len())).rev().find(has_images)
        } else {
            (roll + 1..self.rolls.len()).find(has_images)
        }
    }

    /// Takes the image at `path` out of its roll. Returns the roll and the index it had.
    pub fn remove_image(&mut self, path: &Path) -> Option<(usize, usize)> {
        let folder = path.parent()?;
        let roll = self.rolls.iter().position(|roll| roll.path == folder)?;
        let RollState::Ready { images, .. } = &mut self.rolls[roll].state else {
            return None;
        };
        let index = images.iter().position(|image| image.id.path == path)?;
        let mut remaining = images.to_vec();
        remaining.remove(index);
        let left = remaining.len();
        *images = Arc::from(remaining);
        if let Some((selected_roll, selected)) = self.selection
            && selected_roll == roll
        {
            // The selection stays on the same image, or moves to the one that took its place.
            let moved = if selected > index {
                selected - 1
            } else {
                selected
            };
            self.selection = (left > 0).then(|| (roll, moved.min(left - 1)));
        }
        Some((roll, index))
    }

    pub fn images_of(&self, roll: usize) -> Option<Arc<[ImageEntry]>> {
        match &self.rolls.get(roll)?.state {
            RollState::Ready { images, .. } => Some(Arc::clone(images)),
            _ => None,
        }
    }

    pub fn selected_entry(&self) -> Option<&ImageEntry> {
        let (roll, index) = self.selection?;
        self.rolls.get(roll)?.images().get(index)
    }

    pub fn select(&mut self, roll: usize, index: usize) {
        self.selection = Some((roll, index));
        self.reveal_selection = true;
    }

    pub fn set_sort(&mut self, sort: SortOrder) {
        self.sort = sort;
        self.selection = None;
        for roll in &mut self.rolls {
            if let RollState::Ready { images, .. } = &mut roll.state {
                let mut sorted = images.to_vec();
                listing::sort_images(&mut sorted, sort);
                *images = Arc::from(sorted);
            }
        }
    }

    pub fn toggle_roll(&mut self, roll: usize) {
        if let Some(roll) = self.rolls.get_mut(roll) {
            roll.expanded = !roll.expanded;
        }
    }

    /// Expands every roll, or collapses all if every one is already expanded.
    pub fn toggle_all(&mut self) {
        let expand = self.rolls.iter().any(|roll| !roll.expanded);
        for roll in &mut self.rolls {
            roll.expanded = expand;
        }
    }

    /// Moves the selection `steps` times in `direction`.
    pub fn move_selection(&mut self, direction: Direction, steps: usize) {
        let lengths: Vec<usize> = self.rolls.iter().map(|roll| roll.images().len()).collect();
        let expanded: Vec<bool> = self.rolls.iter().map(|roll| roll.expanded).collect();
        let mut selection = self.selection;
        for _ in 0..steps {
            selection = next_selection(selection, direction, &lengths, &expanded, self.columns);
        }
        if let Some((roll, index)) = selection {
            self.select(roll, index);
        }
    }

    /// Selects the first image of the first roll, or with `last` the last image of the
    /// last roll. Rolls without images do not count.
    pub fn select_end(&mut self, last: bool) {
        let with_images = |roll: &usize| !self.rolls[*roll].images().is_empty();
        let mut rolls = 0..self.rolls.len();
        let target = if last {
            rolls
                .rfind(with_images)
                .map(|roll| (roll, self.rolls[roll].images().len() - 1))
        } else {
            rolls.find(with_images).map(|roll| (roll, 0))
        };
        if let Some((roll, index)) = target {
            self.select(roll, index);
        }
    }
}

fn folder_title(folder: &Path) -> String {
    folder.file_name().map_or_else(
        || folder.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Where the selection lands after one arrow key press.
/// `lengths` and `expanded` describe each roll; rolls without images are skipped.
fn next_selection(
    selection: Option<(usize, usize)>,
    direction: Direction,
    lengths: &[usize],
    expanded: &[bool],
    columns: usize,
) -> Option<(usize, usize)> {
    let columns = columns.max(1);
    let first_roll = lengths.iter().position(|&len| len > 0)?;
    let Some((roll, index)) = selection.filter(|&(r, i)| lengths.get(r).is_some_and(|&l| i < l))
    else {
        return Some((first_roll, 0));
    };
    let previous_roll = (0..roll).rev().find(|&r| lengths[r] > 0);
    let next_roll = (roll + 1..lengths.len()).find(|&r| lengths[r] > 0);
    let len = lengths[roll];
    let column = if expanded[roll] {
        index % columns
    } else {
        index
    };
    // Entering a roll from above or below keeps the column where the roll's layout allows it.
    let enter = |target: usize, from_below: bool| {
        let target_len = lengths[target];
        if expanded[target] && from_below {
            let last_row_start = (target_len - 1) / columns * columns;
            (
                target,
                (last_row_start + column % columns).min(target_len - 1),
            )
        } else {
            (target, column.min(target_len - 1))
        }
    };
    let moved = match direction {
        Direction::Left if index > 0 => (roll, index - 1),
        Direction::Left => previous_roll.map_or((roll, index), |r| (r, lengths[r] - 1)),
        Direction::Right if index + 1 < len => (roll, index + 1),
        Direction::Right => next_roll.map_or((roll, index), |r| (r, 0)),
        Direction::Up if expanded[roll] && index >= columns => (roll, index - columns),
        Direction::Up => previous_roll.map_or((roll, index), |r| enter(r, true)),
        Direction::Down if expanded[roll] && index + columns < len => (roll, index + columns),
        Direction::Down if expanded[roll] && index / columns < (len - 1) / columns => {
            (roll, len - 1)
        }
        Direction::Down => next_roll.map_or((roll, index), |r| enter(r, false)),
    };
    Some(moved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Direction::{Down, Left, Right, Up};

    const LENGTHS: &[usize] = &[10, 0, 5];
    const EXPANDED: &[bool] = &[true, false, false];

    fn step(selection: Option<(usize, usize)>, direction: Direction) -> Option<(usize, usize)> {
        next_selection(selection, direction, LENGTHS, EXPANDED, 4)
    }

    #[test]
    fn first_key_press_selects_the_first_image() {
        assert_eq!(step(None, Right), Some((0, 0)));
        assert_eq!(
            next_selection(None, Right, &[0, 0], &[true, false], 4),
            None
        );
        assert_eq!(step(Some((0, 99)), Left), Some((0, 0)));
    }

    #[test]
    fn left_and_right_cross_roll_boundaries_and_skip_empty_rolls() {
        assert_eq!(step(Some((0, 3)), Right), Some((0, 4)));
        assert_eq!(step(Some((0, 9)), Right), Some((2, 0)));
        assert_eq!(step(Some((2, 0)), Left), Some((0, 9)));
        assert_eq!(step(Some((0, 0)), Left), Some((0, 0)));
        assert_eq!(step(Some((2, 4)), Right), Some((2, 4)));
    }

    #[test]
    fn up_and_down_move_by_rows_inside_an_expanded_roll() {
        assert_eq!(step(Some((0, 1)), Down), Some((0, 5)));
        assert_eq!(step(Some((0, 5)), Up), Some((0, 1)));
        // Row 2 holds only images 8 and 9: moving down from column 3 lands on the last image.
        assert_eq!(step(Some((0, 7)), Down), Some((0, 9)));
    }

    #[test]
    fn up_and_down_leave_the_roll_at_its_edges() {
        assert_eq!(step(Some((0, 9)), Down), Some((2, 1)));
        assert_eq!(step(Some((2, 1)), Up), Some((0, 9)));
        assert_eq!(step(Some((2, 3)), Up), Some((0, 9)));
        assert_eq!(step(Some((0, 2)), Up), Some((0, 2)));
        assert_eq!(step(Some((2, 2)), Down), Some((2, 2)));
    }
}
