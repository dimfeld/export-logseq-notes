use std::borrow::Cow;

fn escape_char(c: char) -> Option<&'static str> {
  match c {
    '>' => Some("&gt;"),
    '<' => Some("&lt;"),
    '&' => Some("&amp;"),
    '\'' => Some("&#39;"),
    '"' => Some("&quot;"),
    _ => None,
  }
}

pub fn escape(input: &str) -> Cow<str> {
  for (i, c) in input.char_indices() {
    if let Some(e) = escape_char(c) {
      let mut output = String::with_capacity(input.len() + e.len());

      // Push all the characters we've already done.
      output.push_str(&input[..i]);
      // Push the one we just escaped
      output.push_str(e);

      // Process the rest of the string right here.
      for c in input[i + 1..].chars() {
        match escape_char(c) {
          Some(e) => output.push_str(e),
          None => output.push(c),
        }
      }

      return Cow::from(output);
    }
  }

  // Nothing to escape so just return the same string.
  Cow::from(input)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn needs_escape() {
    assert_eq!(
      escape("A <string> 'that' \"needs\" &escaping"),
      Cow::Owned::<str>(
        "A &lt;string&gt; &#39;that&#39; &quot;needs&quot; &amp;escaping".to_string()
      )
    );
  }

  #[test]
  fn no_escape() {
    assert_eq!(
      escape("A simple string that needs no escaping"),
      Cow::Borrowed("A simple string that needs no escaping")
    );
  }

  #[test]
  fn unicode() {
    assert_eq!(
      escape(r##"client’s • merkle tree"##),
      Cow::Borrowed(r##"client’s • merkle tree"##)
    )
  }
}
