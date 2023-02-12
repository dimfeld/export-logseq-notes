use std::borrow::Cow;

#[derive(Clone, Debug)]
pub enum StringBuilder<'a> {
    Empty,
    String(Cow<'a, str>),
    Vec(Vec<StringBuilder<'a>>),
}

impl<'a> StringBuilder<'a> {
    pub fn new() -> StringBuilder<'a> {
        StringBuilder::Vec(Vec::new())
    }

    pub fn with_capacity(capacity: usize) -> StringBuilder<'a> {
        StringBuilder::Vec(Vec::with_capacity(capacity))
    }

    pub fn push<T: Into<StringBuilder<'a>>>(&mut self, value: T) {
        match self {
            StringBuilder::Vec(ref mut v) => v.push(value.into()),
            _ => panic!("Tried to push_str on non-vector StringBuilder"),
        }
    }

    fn append(self, output: &mut String) {
        match self {
            StringBuilder::Empty => (),
            StringBuilder::String(s) => output.push_str(&s),
            StringBuilder::Vec(v) => v.into_iter().for_each(|sb| sb.append(output)),
        }
    }

    pub fn build(self) -> String {
        match &self {
            StringBuilder::Empty => String::new(),
            StringBuilder::String(s) => s.to_string(),
            StringBuilder::Vec(_) => {
                let mut output = String::new();
                self.append(&mut output);
                output
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            StringBuilder::Empty => true,
            StringBuilder::String(s) => s.is_empty(),
            StringBuilder::Vec(v) => v.is_empty() || v.iter().all(|s| s.is_empty()),
        }
    }

    pub fn is_blank(&self) -> bool {
        match self {
            StringBuilder::Empty => true,
            StringBuilder::String(s) => s.trim().is_empty(),
            StringBuilder::Vec(v) => v.is_empty() || v.iter().all(|s| s.is_blank()),
        }
    }

    /// Return true if the StringBuilder starts with a prefix. This is a simple
    /// implementation that only looks at the first string component of the StringBuilder.
    pub fn starts_with(&self, prefix: &str) -> bool {
        match self {
            StringBuilder::Empty => false,
            StringBuilder::String(s) => s.starts_with(prefix),
            StringBuilder::Vec(v) => v.first().map_or(false, |s| s.starts_with(prefix)),
        }
    }
}

impl<'a> From<Cow<'a, str>> for StringBuilder<'a> {
    fn from(s: Cow<'a, str>) -> StringBuilder<'a> {
        StringBuilder::String(s)
    }
}

impl<'a> From<String> for StringBuilder<'a> {
    fn from(s: String) -> StringBuilder<'a> {
        StringBuilder::String(Cow::from(s))
    }
}

impl<'a> From<&'a str> for StringBuilder<'a> {
    fn from(s: &'a str) -> StringBuilder<'a> {
        StringBuilder::String(Cow::from(s))
    }
}

// impl<'a> From<Vec<StringBuilder<'a>>> for StringBuilder<'a> {
//   fn from(s: Vec<StringBuilder<'a>>) -> StringBuilder<'a> {
//     StringBuilder::Vec(s)
//   }
// }

impl<'a, T: Into<StringBuilder<'a>>> From<Vec<T>> for StringBuilder<'a> {
    fn from(s: Vec<T>) -> StringBuilder<'a> {
        StringBuilder::Vec(s.into_iter().map(|e| e.into()).collect::<Vec<_>>())
    }
}

impl<'a, ITEM: Into<StringBuilder<'a>>> FromIterator<ITEM> for StringBuilder<'a> {
    fn from_iter<T: IntoIterator<Item = ITEM>>(iter: T) -> Self {
        StringBuilder::Vec(iter.into_iter().map(|e| e.into()).collect::<Vec<_>>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        assert_eq!(StringBuilder::Empty.build(), "");
    }

    #[test]
    fn simple_string() {
        assert_eq!(StringBuilder::from("abcdef").build(), "abcdef");
    }

    #[test]
    fn vec_of_strings() {
        assert_eq!(
            StringBuilder::from(vec!["abcdef", "  ghi", "-klm"]).build(),
            "abcdef  ghi-klm"
        );
    }

    #[test]
    fn nested() {
        let sb = StringBuilder::Vec(vec![
            StringBuilder::from("<h1>"),
            StringBuilder::from(vec![
                StringBuilder::from("Some"),
                StringBuilder::from(" text"),
            ]),
            StringBuilder::from("</h1>"),
        ]);

        assert_eq!(sb.build(), "<h1>Some text</h1>");
    }
}
