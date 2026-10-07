use rand::seq::SliceRandom;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

pub fn discover(directories: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut pending: Vec<PathBuf> = directories.to_vec();
    while let Some(path) = pending.pop() {
        let Ok(canonical) = path.canonicalize() else {
            continue;
        };
        if !seen.insert(canonical.clone()) {
            continue;
        }
        if canonical.is_dir() {
            if let Ok(entries) = fs::read_dir(&canonical) {
                pending.extend(entries.flatten().map(|entry| entry.path()));
            }
        } else if is_photo(&canonical) {
            found.push(canonical);
        }
    }
    found.sort();
    found
}

fn is_photo(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "tif" | "tiff"
            )
        })
        .unwrap_or(false)
}

pub struct ShuffleBag {
    paths: Vec<PathBuf>,
    remaining: Vec<usize>,
    last: Option<usize>,
}

impl ShuffleBag {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self {
            paths,
            remaining: Vec::new(),
            last: None,
        }
    }

    pub fn next(&mut self) -> Option<PathBuf> {
        if self.paths.is_empty() {
            return None;
        }
        if self.remaining.is_empty() {
            self.remaining = (0..self.paths.len()).collect();
            self.remaining.shuffle(&mut rand::thread_rng());
            if self.remaining.len() > 1 && self.remaining.last() == self.last.as_ref() {
                let last = self.remaining.len() - 1;
                self.remaining.swap(last, 0);
            }
        }
        let index = self.remaining.pop()?;
        self.last = Some(index);
        Some(self.paths[index].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_bag_uses_every_photo_before_repeating() {
        let mut bag = ShuffleBag::new((0..7).map(|n| PathBuf::from(n.to_string())).collect());
        let mut previous = None;
        for _ in 0..100 {
            let batch: Vec<_> = (0..7).map(|_| bag.next().unwrap()).collect();
            assert_eq!(batch.iter().collect::<HashSet<_>>().len(), 7);
            if let Some(previous) = previous {
                assert_ne!(batch[0], previous);
            }
            previous = batch.last().cloned();
        }
    }

    #[test]
    fn extension_filter_is_case_insensitive() {
        assert!(is_photo(Path::new("photo.JPEG")));
        assert!(!is_photo(Path::new("notes.txt")));
    }
}
