//! Tree width, directory defaults and README annotations at parser boundaries.

use super::tree::{readme_rows, tree_lines};
use crate::checks::private_directories::private_directory;
use std::collections::BTreeMap;
use std::{env, fs, slice};

#[test]
fn long_entries_keep_two_spaces_before_the_explanation() {
    let name = "x".repeat(60);
    let lines = tree_lines(
        env::temp_dir().as_path(),
        slice::from_ref(&name),
        &BTreeMap::new(),
        &BTreeMap::new(),
    );
    assert_eq!(lines[1], format!("└── {name}  # File: {name}"));
}

#[test]
fn directory_defaults_match_the_path_before_its_name() {
    let paths = ["docs/a.bin", "other/docs/b.bin", "custom-name/c.bin"].map(str::to_owned);
    let lines = tree_lines(
        env::temp_dir().as_path(),
        &paths,
        &BTreeMap::new(),
        &BTreeMap::new(),
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("├── docs/") && line.ends_with("# Documentation"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("└── docs/") && line.ends_with("# Documentation"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("custom-name/") && line.ends_with("# Custom name"))
    );
}

#[test]
fn readme_annotations_reject_empty_cells_and_attribute_prefixes() {
    let root = private_directory(env::temp_dir().to_str().unwrap(), "guide-rows").unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("docs/README.md"),
        concat!(
            "| `` | Empty path |\n| `a|b` | Invalid path |\n| `blank.png` |  |\n",
            "| `valid.png` | Valid table. |\n",
            "<image src=\"image.png\" alt=\"Not a tag\">\n",
            "<img_extra src=\"extra.png\" alt=\"Not a tag\">\n",
            "<img data-src=\"dash.png\" alt=\"Dash attribute\">\n",
            "<img xsrc=\"letter.png\" alt=\"Not src\">\n",
            "<img _src=\"underscore.png\" alt=\"Not src\">\n",
            "<img src=\"real.png\" alt=\"Real image.\">\n",
        ),
    )
    .unwrap();
    let paths = [
        "docs/README.md",
        "docs/dash.png",
        "docs/extra.png",
        "docs/image.png",
        "docs/letter.png",
        "docs/real.png",
        "docs/underscore.png",
        "docs/valid.png",
    ]
    .map(str::to_owned);
    assert_eq!(
        readme_rows(&root, &paths),
        BTreeMap::from([
            ("docs/dash.png".to_owned(), "Dash attribute".to_owned()),
            ("docs/real.png".to_owned(), "Real image".to_owned()),
            ("docs/valid.png".to_owned(), "Valid table".to_owned()),
        ])
    );
    fs::remove_dir_all(root).unwrap();
}
