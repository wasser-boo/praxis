use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Tag {
    Done,
    Next,
    Feedback { message: String },
    Push { path: String },
    Pop,
    Path { path: String },
    Mode { mode: String },
    Set { key: String, value: String },
    Learn { fact: String },
}

pub fn parse_tags(content: &str) -> (String, Vec<Tag>) {
    let mut tags = Vec::new();
    let mut clean_content = content.to_string();

    if content.contains("§done") {
        tags.push(Tag::Done);
        clean_content = clean_content.replace("§done", "");
    }

    if content.contains("§next") {
        tags.push(Tag::Next);
        clean_content = clean_content.replace("§next", "");
    }

    let feedback_re = regex::Regex::new(r"§feedback\{([^}]+)\}").unwrap();
    for cap in feedback_re.captures_iter(content) {
        tags.push(Tag::Feedback {
            message: cap[1].to_string(),
        });
        clean_content = clean_content.replace(&cap[0], "");
    }

    let push_re = regex::Regex::new(r"§push\{([^}]+)\}").unwrap();
    for cap in push_re.captures_iter(content) {
        tags.push(Tag::Push {
            path: cap[1].to_string(),
        });
        clean_content = clean_content.replace(&cap[0], "");
    }

    if content.contains("§pop") {
        tags.push(Tag::Pop);
        clean_content = clean_content.replace("§pop", "");
    }

    let path_re = regex::Regex::new(r"§path\{([^}]+)\}").unwrap();
    for cap in path_re.captures_iter(content) {
        tags.push(Tag::Path {
            path: cap[1].to_string(),
        });
        clean_content = clean_content.replace(&cap[0], "");
    }

    let mode_re = regex::Regex::new(r"§mode\{([^}]+)\}").unwrap();
    for cap in mode_re.captures_iter(content) {
        tags.push(Tag::Mode {
            mode: cap[1].to_string(),
        });
        clean_content = clean_content.replace(&cap[0], "");
    }

    let set_re = regex::Regex::new(r"§set\{([^=]+)=([^}]+)\}").unwrap();
    for cap in set_re.captures_iter(content) {
        tags.push(Tag::Set {
            key: cap[1].to_string(),
            value: cap[2].to_string(),
        });
        clean_content = clean_content.replace(&cap[0], "");
    }

    let learn_re = regex::Regex::new(r"§learn\{([^}]+)\}").unwrap();
    for cap in learn_re.captures_iter(content) {
        tags.push(Tag::Learn {
            fact: cap[1].to_string(),
        });
        clean_content = clean_content.replace(&cap[0], "");
    }

    clean_content = regex::Regex::new(r"\n{3,}")
        .unwrap()
        .replace_all(&clean_content, "\n\n")
        .to_string();

    (clean_content.trim().to_string(), tags)
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn test_parse_done() {
        let (clean, tags) = parse_tags("Hello §done");
        assert_eq!(clean, "Hello");
        assert_eq!(tags, vec![Tag::Done]);
    }

    #[test]
    fn test_parse_next() {
        let (clean, tags) = parse_tags("Next step §next");
        assert_eq!(clean, "Next step");
        assert_eq!(tags, vec![Tag::Next]);
    }

    #[test]
    fn test_parse_feedback() {
        let (clean, tags) = parse_tags("Hello §feedback{good job}");
        assert_eq!(clean, "Hello");
        assert_eq!(
            tags,
            vec![Tag::Feedback {
                message: "good job".to_string()
            }]
        );
    }

    #[test]
    fn test_parse_push() {
        let (clean, tags) = parse_tags("§push{tasks/code}");
        assert_eq!(clean, "");
        assert_eq!(
            tags,
            vec![Tag::Push {
                path: "tasks/code".to_string()
            }]
        );
    }

    #[test]
    fn test_parse_set() {
        let (clean, tags) = parse_tags("§set{mode=agent}");
        assert_eq!(clean, "");
        assert_eq!(
            tags,
            vec![Tag::Set {
                key: "mode".to_string(),
                value: "agent".to_string()
            }]
        );
    }

    #[test]
    fn test_parse_multiple_tags() {
        let (clean, tags) = parse_tags("Done now §done §next §learn{Rust is great}");
        assert_eq!(clean, "Done now");
        assert_eq!(tags.len(), 3);
        assert!(tags.contains(&Tag::Done));
        assert!(tags.contains(&Tag::Next));
        assert!(
            tags.contains(&Tag::Learn {
                fact: "Rust is great".to_string()
            })
        );
    }

    #[test]
    fn test_no_tags() {
        let (clean, tags) = parse_tags("Just plain text");
        assert_eq!(clean, "Just plain text");
        assert!(tags.is_empty());
    }

    #[test]
    fn test_clean_extra_newlines() {
        let (clean, _) = parse_tags("Hello\n\n\n\nWorld");
        assert_eq!(clean, "Hello\n\nWorld");
    }
}
