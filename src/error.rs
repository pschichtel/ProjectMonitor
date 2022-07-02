use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub enum QueryError {
    HttpError(u16),
    NoData,
}

impl Display for QueryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryError::HttpError(code) => f.write_fmt(format_args!("http error: {}", code)),
            QueryError::NoData => f.write_str("no data"),
        }
    }
}

impl Error for QueryError {
    fn cause(&self) -> Option<&dyn Error> {
        None
    }
}