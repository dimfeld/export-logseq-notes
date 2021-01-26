use itertools::Itertools;
use std::borrow::Cow;

/// Given two paths and an optional base directory, calculate the appropriate link.
/// If `base` is supplied, the generated URL will always be an absolute URL
/// starting with `base`.
pub fn link_path<'a>(src: &str, dest: &'a str, base: Option<&str>) -> Cow<'a, str> {
    if let Some(b) = base {
        return Cow::from(format!("{}{}", b, dest));
    }

    let common = src
        .split('/')
        .zip(dest.split('/'))
        .take_while(|(s, d)| s == d)
        .count();

    let src_dir_size = src.split('/').count() - 1;
    let dest_dir_size = dest.split('/').count() - 1;
    if (src_dir_size == dest_dir_size && dest_dir_size == common) || common == dest_dir_size + 1 {
        // They're in the same directory, or the same file
        return Cow::from(dest.split('/').last().unwrap_or_default());
    }

    let step_up = src_dir_size - common;
    let dots = std::iter::repeat("..").take(step_up);
    let step_in = dest.split('/').skip(common);

    let result = dots.chain(step_in).join("/");
    Cow::from(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_dir() {
        assert_eq!(link_path("a", "b", None), Cow::Borrowed("b"));
    }

    #[test]
    fn same_file() {
        assert_eq!(link_path("d/a", "d/a", None), Cow::Borrowed("a"));
    }

    #[test]
    fn same_dir() {
        assert_eq!(link_path("d/a", "d/b", None), Cow::Borrowed("b"));
    }

    #[test]
    fn same_dir2() {
        assert_eq!(link_path("d/c/a", "d/c/b", None), Cow::Borrowed("b"));
    }

    #[test]
    fn prev_dir() {
        assert_eq!(
            link_path("d/c/a", "d/b", None),
            Cow::from("../b".to_string())
        );
    }

    #[test]
    fn src_in_root() {
        assert_eq!(
            link_path("a", "d/g/h", None),
            Cow::from("d/g/h".to_string())
        );
    }

    #[test]
    fn dest_in_root() {
        assert_eq!(
            link_path("d/c/a", "b", None),
            Cow::from("../../b".to_string())
        );
    }

    #[test]
    fn next_dir() {
        assert_eq!(
            link_path("d/a", "d/c/b", None),
            Cow::from("c/b".to_string())
        );
    }

    #[test]
    fn next_dir2() {
        assert_eq!(
            link_path("d/a", "d/f/c/b", None),
            Cow::from("f/c/b".to_string())
        );
    }

    #[test]
    fn sibling_dir() {
        assert_eq!(
            link_path("d/a", "c/b", None),
            Cow::from("../c/b".to_string())
        );
    }

    #[test]
    fn sibling_dir2() {
        assert_eq!(
            link_path("f/d/a", "g/c/b", None),
            Cow::from("../../g/c/b".to_string())
        );
    }

    #[test]
    fn sibling_dir3() {
        assert_eq!(
            link_path("g/d/a", "g/c/b", None),
            Cow::from("../c/b".to_string())
        );
    }
}
