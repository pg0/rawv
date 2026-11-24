//! Natural, case-insensitive name ordering: `IMG_2` sorts before `IMG_10`.

use std::cmp::Ordering;

pub fn compare(a: &str, b: &str) -> Ordering {
    let mut left = a.chars().peekable();
    let mut right = b.chars().peekable();
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(l), Some(r)) if l.is_ascii_digit() && r.is_ascii_digit() => {
                let left_run = take_digits(&mut left);
                let right_run = take_digits(&mut right);
                let ordering = compare_digit_runs(&left_run, &right_run);
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (Some(l), Some(r)) => {
                let ordering = l.to_lowercase().cmp(r.to_lowercase());
                if ordering != Ordering::Equal {
                    return ordering;
                }
                left.next();
                right.next();
            }
        }
    }
}

fn take_digits(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut run = String::new();
    while let Some(c) = chars.peek().copied().filter(char::is_ascii_digit) {
        run.push(c);
        chars.next();
    }
    run
}

fn compare_digit_runs(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_sort_by_value() {
        let mut names = vec!["IMG_10.jpg", "IMG_2.jpg", "img_1.jpg", "IMG_0003.jpg"];
        names.sort_by(|a, b| compare(a, b));
        assert_eq!(
            names,
            ["img_1.jpg", "IMG_2.jpg", "IMG_0003.jpg", "IMG_10.jpg"]
        );
    }

    #[test]
    fn letters_ignore_case_and_prefixes_sort_first() {
        assert_eq!(compare("alpha", "Beta"), Ordering::Less);
        assert_eq!(compare("roll", "roll 2"), Ordering::Less);
        assert_eq!(compare("a", "a"), Ordering::Equal);
        assert_ne!(compare("A", "a"), Ordering::Equal);
    }

    #[test]
    fn very_long_digit_runs_do_not_overflow() {
        let a = "scan_99999999999999999999999999999999.png";
        let b = "scan_100000000000000000000000000000000.png";
        assert_eq!(compare(a, b), Ordering::Less);
    }
}
