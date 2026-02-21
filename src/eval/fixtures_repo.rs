use crate::eval::tasks::Fixture;

pub fn workspace_refactor_fixtures() -> Vec<Fixture> {
    vec![
        Fixture::WriteFile {
            path: "Cargo.toml".to_string(),
            content: "[workspace]\nmembers = [\"crates/libcore\", \"crates/app\"]\nresolver = \"2\"\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "README.md".to_string(),
            content: "# Workspace Fixture\n\nTODO: add refactor note.\n".to_string(),
        },
        Fixture::WriteFile {
            path: "crates/libcore/Cargo.toml".to_string(),
            content: "[package]\nname = \"libcore\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "crates/libcore/src/lib.rs".to_string(),
            content: "pub fn combine(a: i32, b: i32) -> i32 {\n    // TODO: fix implementation and refactor signature\n    a - b\n}\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "crates/libcore/tests/basic.rs".to_string(),
            content: "use libcore::combine;\n\n#[test]\nfn combine_adds_values() {\n    assert_eq!(combine(2, 3), 5);\n}\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "crates/app/Cargo.toml".to_string(),
            content: "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nlibcore = { path = \"../libcore\" }\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "crates/app/src/main.rs".to_string(),
            content: "fn main() {\n    let v = libcore::combine(10, 5);\n    println!(\"{v}\");\n}\n"
                .to_string(),
        },
    ]
}

pub fn cli_bugfix_fixtures() -> Vec<Fixture> {
    vec![
        Fixture::WriteFile {
            path: "Cargo.toml".to_string(),
            content: "[package]\nname = \"cli_bugfix\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "src/lib.rs".to_string(),
            content: "pub fn parse_count(input: &str) -> Result<u32, String> {\n    // Bug: this rejects inputs with surrounding spaces.\n    if input.chars().all(|c| c.is_ascii_digit()) {\n        input.parse::<u32>().map_err(|e| e.to_string())\n    } else {\n        Err(\"invalid number\".to_string())\n    }\n}\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "src/main.rs".to_string(),
            content: "fn main() {\n    let _ = cli_bugfix::parse_count(\"7\");\n}\n".to_string(),
        },
        Fixture::WriteFile {
            path: "tests/regression.rs".to_string(),
            content: "use cli_bugfix::parse_count;\n\n#[test]\nfn parses_simple_count() {\n    assert_eq!(parse_count(\"12\").unwrap(), 12);\n}\n\n#[test]\nfn parses_spaced_count() {\n    assert_eq!(parse_count(\" 12 \").unwrap(), 12);\n}\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "README.md".to_string(),
            content: "# CLI bugfix fixture\n".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{cli_bugfix_fixtures, workspace_refactor_fixtures};

    #[test]
    fn workspace_fixture_is_deterministic_and_large_enough() {
        let a = workspace_refactor_fixtures();
        let b = workspace_refactor_fixtures();
        assert_eq!(a.len(), b.len());
        assert!(a.len() >= 5);
        assert!(a.iter().any(|f| matches!(f, crate::eval::tasks::Fixture::WriteFile { path, .. } if path == "Cargo.toml")));
    }

    #[test]
    fn cli_fixture_contains_regression_tests() {
        let f = cli_bugfix_fixtures();
        let has_tests = f.iter().any(|fx| matches!(fx, crate::eval::tasks::Fixture::WriteFile { path, content } if path == "tests/regression.rs" && content.contains("parses_spaced_count")));
        assert!(has_tests);
    }
}
