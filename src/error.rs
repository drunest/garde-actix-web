//! Error exposed by actix-web-garde
//!
//! Custom error handlers (provided through the divers configs) should map from an `actix_web_garde::error::Error` to an `actix_web::error::Error`
use actix_web::error::{JsonPayloadError, PathError, QueryPayloadError, UrlencodedError};
use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
use garde::Errors;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
  #[error("Validation error: {0}")]
  ValidationError(Errors),
  #[error("Payload error: {0}")]
  JsonPayloadError(JsonPayloadError),
  #[error("Payload error: {0}")]
  QueryPayloadError(QueryPayloadError),
  #[error("Path error: {0}")]
  PathError(PathError),
  #[error("Urlencoded error: {0}")]
  UrlencodedError(UrlencodedError),
  #[cfg(feature = "serde_qs")]
  #[error("Query error: {0}")]
  QsError(serde_qs::Error),
}

impl From<Errors> for Error {
  fn from(error: Errors) -> Self {
    Self::ValidationError(error)
  }
}

impl From<JsonPayloadError> for Error {
  fn from(error: JsonPayloadError) -> Self {
    Self::JsonPayloadError(error)
  }
}

impl From<QueryPayloadError> for Error {
  fn from(error: QueryPayloadError) -> Self {
    Self::QueryPayloadError(error)
  }
}

impl From<PathError> for Error {
  fn from(error: PathError) -> Self {
    Self::PathError(error)
  }
}

impl From<UrlencodedError> for Error {
  fn from(error: UrlencodedError) -> Self {
    Self::UrlencodedError(error)
  }
}

#[cfg(feature = "serde_qs")]
impl From<serde_qs::Error> for Error {
  fn from(error: serde_qs::Error) -> Self {
    Self::QsError(error)
  }
}

impl ResponseError for Error {
  fn status_code(&self) -> StatusCode {
    match self {
      Error::ValidationError(_) => StatusCode::BAD_REQUEST,
      Error::JsonPayloadError(e) => e.status_code(),
      Error::QueryPayloadError(e) => e.status_code(),
      Error::PathError(e) => e.status_code(),
      Error::UrlencodedError(e) => e.status_code(),
      #[cfg(feature = "serde_qs")]
      Error::QsError(_) => StatusCode::BAD_REQUEST,
    }
  }

  fn error_response(&self) -> HttpResponse {
    HttpResponse::build(self.status_code()).body(format!("{}", *self))
  }
}
