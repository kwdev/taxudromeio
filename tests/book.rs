use std::{
    fs,
    path::{Path, PathBuf},
};

use anchor_book::{
    Chapter,
    checking::{self, Outcome},
    read_records,
    roadmaps::read_roadmaps,
};

fn markdown(directory: &Path, paths: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            markdown(&path, paths);
        } else if path.extension().is_some_and(|extension| extension == "md")
            && path.file_name().unwrap() != "SUMMARY.md"
        {
            paths.push(path);
        }
    }
}

#[test]
fn book_structure_records_and_roadmaps_conform_to_the_pinned_engine() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let report =
        checking::check(&root.join("anchorbook.toml")).unwrap_or_else(|error| error.into_report());
    assert_eq!(
        report.outcome(),
        Outcome::Passed,
        "{}",
        anchor_book::adapters::check::format_report(&report)
    );
    let mut paths = Vec::new();
    markdown(&root.join("book"), &mut paths);
    paths.sort();
    let contents = paths
        .iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect::<Vec<_>>();
    let chapters = paths
        .iter()
        .zip(&contents)
        .map(|(path, text)| Chapter::new(path, text))
        .collect::<Vec<_>>();
    let records = read_records(&chapters).unwrap();
    assert!(!records.is_empty());
    let roadmaps = chapters
        .into_iter()
        .filter(|c| {
            c.path
                .starts_with(root.join("book/implementation-roadmaps"))
        })
        .collect::<Vec<_>>();
    assert_eq!(read_roadmaps(&roadmaps, &records).unwrap().len(), 2);
}
