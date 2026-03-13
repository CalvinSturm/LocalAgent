use crate::eval::tasks::Fixture;

pub fn code_investigation_fixtures() -> Vec<Fixture> {
    vec![
        Fixture::WriteFile {
            path: "Cargo.toml".to_string(),
            content: "[package]\nname = \"code_investigation\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "src/main.rs".to_string(),
            content: "mod cli_dispatch;\n\nfn main() {\n    cli_dispatch::run_cli();\n}\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "src/cli_dispatch.rs".to_string(),
            content: "pub fn run_cli() {\n    println!(\"dispatch\");\n}\n".to_string(),
        },
        Fixture::WriteFile {
            path: "src/runtime.rs".to_string(),
            content: "pub fn run_agent() {\n    println!(\"agent\");\n}\n".to_string(),
        },
        Fixture::WriteFile {
            path: "README.md".to_string(),
            content:
                "# Code investigation fixture\n\nFind the entrypoint and the CLI dispatch function.\n"
                    .to_string(),
        },
    ]
}

pub fn single_file_bugfix_fixtures() -> Vec<Fixture> {
    vec![
        Fixture::WriteFile {
            path: "Cargo.toml".to_string(),
            content: "[package]\nname = \"single_file_bugfix\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "src/math.rs".to_string(),
            content: "pub fn total(a: i32, b: i32) -> i32 {\n    a - b\n}\n".to_string(),
        },
        Fixture::WriteFile {
            path: "src/main.rs".to_string(),
            content: "mod math;\n\nfn main() {\n    println!(\"{}\", math::total(2, 3));\n}\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "README.md".to_string(),
            content: "# Single-file bugfix fixture\n".to_string(),
        },
    ]
}

pub fn closeout_quality_bugfix_fixtures() -> Vec<Fixture> {
    vec![
        Fixture::WriteFile {
            path: "Cargo.toml".to_string(),
            content: "[package]\nname = \"closeout_quality_bugfix\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "src/lib.rs".to_string(),
            content: "pub mod math;\n".to_string(),
        },
        Fixture::WriteFile {
            path: "src/math.rs".to_string(),
            content: "pub fn total(a: i32, b: i32) -> i32 {\n    a - b\n}\n".to_string(),
        },
        Fixture::WriteFile {
            path: "tests/regression.rs".to_string(),
            content: "use closeout_quality_bugfix::math::total;\n\n#[test]\nfn total_adds_values() {\n    assert_eq!(total(2, 3), 5);\n}\n".to_string(),
        },
        Fixture::WriteFile {
            path: "README.md".to_string(),
            content: "# Closeout quality bugfix fixture\n".to_string(),
        },
    ]
}

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

pub fn inspect_before_edit_fixtures() -> Vec<Fixture> {
    vec![
        Fixture::WriteFile {
            path: "Cargo.toml".to_string(),
            content:
                "[package]\nname = \"inspect_before_edit\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
                    .to_string(),
        },
        Fixture::WriteFile {
            path: "src/main.rs".to_string(),
            content:
                "mod messages;\n\nfn main() {\n    println!(\"{}\", messages::greeting());\n}\n"
                    .to_string(),
        },
        Fixture::WriteFile {
            path: "src/messages.rs".to_string(),
            content: "pub fn greeting() -> &'static str {\n    \"helo\"\n}\n".to_string(),
        },
        Fixture::WriteFile {
            path: "src/unused.rs".to_string(),
            content: "pub const NOISE: &str = \"ignore me\";\n".to_string(),
        },
        Fixture::WriteFile {
            path: "README.md".to_string(),
            content: "# Inspect-before-edit fixture\n\nFind the real greeting definition before editing.\n"
                .to_string(),
        },
    ]
}

pub fn recovery_bugfix_fixtures() -> Vec<Fixture> {
    vec![
        Fixture::WriteFile {
            path: "Cargo.toml".to_string(),
            content:
                "[package]\nname = \"recovery_bugfix\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
                    .to_string(),
        },
        Fixture::WriteFile {
            path: "src/main.rs".to_string(),
            content: "fn main() {\n    let _ = recovery_bugfix::parse_count(\"7\");\n}\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "src/lib.rs".to_string(),
            content: "pub mod parser;\n\npub use parser::parse_count;\n".to_string(),
        },
        Fixture::WriteFile {
            path: "src/parser.rs".to_string(),
            content: "pub fn parse_count(input: &str) -> Result<u32, String> {\n    if input.chars().all(|c| c.is_ascii_digit()) {\n        input.parse::<u32>().map_err(|e| e.to_string())\n    } else {\n        Err(\"invalid number\".to_string())\n    }\n}\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "tests/regression.rs".to_string(),
            content: "use recovery_bugfix::parse_count;\n\n#[test]\nfn parses_simple_count() {\n    assert_eq!(parse_count(\"12\").unwrap(), 12);\n}\n\n#[test]\nfn parses_spaced_count() {\n    assert_eq!(parse_count(\" 12 \").unwrap(), 12);\n}\n"
                .to_string(),
        },
        Fixture::WriteFile {
            path: "README.md".to_string(),
            content: "# Recovery bugfix fixture\n\nThe parser lives in src/parser.rs.\n".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        cli_bugfix_fixtures, closeout_quality_bugfix_fixtures, code_investigation_fixtures,
        inspect_before_edit_fixtures, recovery_bugfix_fixtures, single_file_bugfix_fixtures,
        workspace_refactor_fixtures,
    };

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

    #[test]
    fn inspect_before_edit_fixture_contains_messages_file() {
        let f = inspect_before_edit_fixtures();
        let has_target = f.iter().any(|fx| matches!(fx, crate::eval::tasks::Fixture::WriteFile { path, content } if path == "src/messages.rs" && content.contains("\"helo\"")));
        assert!(has_target);
    }

    #[test]
    fn recovery_bugfix_fixture_places_parser_in_nested_file() {
        let f = recovery_bugfix_fixtures();
        let has_parser = f.iter().any(|fx| matches!(fx, crate::eval::tasks::Fixture::WriteFile { path, content } if path == "src/parser.rs" && content.contains("parse_count")));
        assert!(has_parser);
    }

    #[test]
    fn code_investigation_fixture_contains_entrypoint_and_dispatch() {
        let f = code_investigation_fixtures();
        let has_main = f.iter().any(|fx| matches!(fx, crate::eval::tasks::Fixture::WriteFile { path, content } if path == "src/main.rs" && content.contains("cli_dispatch::run_cli")));
        let has_dispatch = f.iter().any(|fx| matches!(fx, crate::eval::tasks::Fixture::WriteFile { path, content } if path == "src/cli_dispatch.rs" && content.contains("pub fn run_cli")));
        assert!(has_main);
        assert!(has_dispatch);
    }

    #[test]
    fn single_file_bugfix_fixture_contains_math_target() {
        let f = single_file_bugfix_fixtures();
        let has_math = f.iter().any(|fx| matches!(fx, crate::eval::tasks::Fixture::WriteFile { path, content } if path == "src/math.rs" && content.contains("a - b")));
        assert!(has_math);
    }

    #[test]
    fn closeout_quality_fixture_contains_regression_test() {
        let f = closeout_quality_bugfix_fixtures();
        let has_test = f.iter().any(|fx| matches!(fx, crate::eval::tasks::Fixture::WriteFile { path, content } if path == "tests/regression.rs" && content.contains("total_adds_values")));
        assert!(has_test);
    }
}
