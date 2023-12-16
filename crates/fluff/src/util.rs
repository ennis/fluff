use regex::Regex;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Given a path of the form `foo####.ext`, with `####` being a frame number, returns a list of all files in the same directory
/// that follow the same pattern, sorted by frame number.
///
/// # Limitations
///
/// This won't work if the stem (the part before the frame number) contains digits.
pub fn resolve_file_sequence(path: &Path) -> io::Result<Vec<(usize, PathBuf)>> {
    // TODO error handling for invalid paths
    let file_name = path.file_name().unwrap().to_str().unwrap();
    let parent_dir = path.parent().unwrap();

    let mut results = vec![];

    let re = Regex::new(r"(\D*)(\d+)\.(\w*)").unwrap();

    if let Some(c) = re.captures(&file_name) {
        let stem = c.get(1).unwrap().as_str();
        let ext = c.get(3).unwrap().as_str();
        //eprintln!("loading {stem}####.{ext}");
        let files_in_directory = fs::read_dir(parent_dir)?;
        for entry in files_in_directory {
            if let Ok(entry) = entry {
                if let Some(name) = entry.file_name().to_str() {
                    if let Some(c) = re.captures(&name) {
                        //eprintln!("candidate: {}", name);
                        let candidate_stem = c.get(1).unwrap().as_str();
                        let candidate_frame = c.get(2).unwrap().as_str().parse::<usize>().unwrap();
                        let candidate_ext = c.get(3).unwrap().as_str();

                        if candidate_stem == stem && candidate_ext == ext {
                            results.push((candidate_frame, entry.path()));
                        }
                    }
                }
            }
        }
        results.sort_by_key(|a| a.0);
        Ok(results)
    } else {
        Ok(vec![(0, path.to_path_buf())])
    }
}
