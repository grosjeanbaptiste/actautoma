//! Test double shared by the adapters: a scripted [`Runner`].

use crate::forge::{Error, Runner};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

pub type Call = (String, Vec<String>, Option<PathBuf>);

/// Scripted runner: records every call, answers from a queue.
pub struct Fake {
    calls: RefCell<Vec<Call>>,
    answers: RefCell<Vec<Result<String, Error>>>,
}

impl Fake {
    pub fn answering(answers: Vec<Result<String, Error>>) -> Self {
        Fake {
            calls: RefCell::default(),
            answers: RefCell::new(answers),
        }
    }
    pub fn calls_len(&self) -> usize {
        self.calls.borrow().len()
    }
    pub fn call(&self, i: usize) -> Call {
        self.calls.borrow()[i].clone()
    }
}

impl Runner for &Fake {
    fn run(&self, program: &str, args: &[&str], dir: Option<&Path>) -> Result<String, Error> {
        self.calls.borrow_mut().push((
            program.to_string(),
            args.iter().map(|a| a.to_string()).collect(),
            dir.map(Path::to_path_buf),
        ));
        self.answers.borrow_mut().remove(0)
    }
}
