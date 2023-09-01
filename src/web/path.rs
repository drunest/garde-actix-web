use actix_router::PathDeserializer;
use actix_web::dev::Payload;
use actix_web::error::{ErrorNotFound, PathError};
use actix_web::web::Data;
use actix_web::{Error, FromRequest, HttpRequest};
use std::sync::Arc;

use crate::validate_for_request;
use derive_more::{AsRef, Deref, DerefMut, Display, From};
use futures::future::{err, ok, Ready};
use garde::Validate;
use serde::de::DeserializeOwned;
use serde::Deserialize;

/// Drop in replacement for [actix_web::web::Path](https://docs.rs/actix-web/latest/actix_web/web/struct.Path.html)
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Deref, DerefMut, AsRef, Display, From)]
pub struct Path<T>(T);

impl<T> Path<T> {
  pub fn into_inner(self) -> T {
    self.0
  }
}

impl<T> FromRequest for Path<T>
where
  T: DeserializeOwned + Validate + 'static,
  T::Context: Default,
{
  type Error = Error;
  type Future = Ready<Result<Self, Self::Error>>;

  #[inline]
  fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
    let req_copy = req.clone();
    let error_handler = req
      .app_data::<PathConfig>()
      .or_else(|| req.app_data::<Data<PathConfig>>().map(Data::get_ref))
      .and_then(|c| c.err_handler.clone());

    Deserialize::deserialize(PathDeserializer::new(req.match_info()))
      .map_err(|e| {
        let e = PathError::Deserialize(e);
        crate::error::Error::PathError(e)
      })
      .and_then(|data: T| {
        let req = req_copy;
        validate_for_request(data, &req)
      })
      .map(|val| ok(Path(val)))
      .unwrap_or_else(move |e| {
        log::debug!(
          "Failed during Path extractor deserialization. \
                         Request path: {:?}",
          req.path()
        );

        let e = if let Some(error_handler) = error_handler {
          (error_handler)(e, req)
        } else {
          ErrorNotFound(e)
        };

        err(e)
      })
  }
}

/// Replacement for [actix_web::web::PathConfig](https://docs.rs/actix-web/latest/actix_web/web/struct.PathConfig.html)
/// Error handler must map from an `actix_web_garde::error::Error`
#[derive(Clone, Default)]
pub struct PathConfig {
  #[allow(clippy::type_complexity)]
  err_handler: Option<Arc<dyn Fn(crate::error::Error, &HttpRequest) -> Error + Send + Sync>>,
}

impl PathConfig {
  pub fn error_handler<F>(mut self, f: F) -> Self
  where
    F: Fn(crate::error::Error, &HttpRequest) -> Error + Send + Sync + 'static,
  {
    self.err_handler = Some(Arc::new(f));
    self
  }
}

#[cfg(test)]
mod test {
  use crate::web::{Path, PathConfig};
  use actix_http::StatusCode;
  use actix_web::error::InternalError;
  use actix_web::test::{call_service, init_service, TestRequest};
  use actix_web::web::{post, resource};
  use actix_web::{App, HttpResponse};
  use garde::Validate;
  use serde::{Deserialize, Serialize};
  use std::fmt;

  #[derive(Debug, PartialEq, Validate, Serialize, Deserialize)]
  struct PathData {
    #[garde(range(min = 18, max = 28))]
    age: u8,
  }

  impl fmt::Display for PathData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
      write!(f, "{{ age: {} }}", self.age)
    }
  }

  #[derive(Debug, PartialEq, Validate, Serialize, Deserialize)]
  #[garde(context(NumberContext))]
  struct PathDataWithContext {
    #[garde(custom(is_big_enough))]
    age: u8,
  }

  impl fmt::Display for PathDataWithContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
      write!(f, "{{ age: {} }}", self.age)
    }
  }

  #[derive(Default, Debug)]
  struct NumberContext {
    min: u8,
  }

  fn is_big_enough(value: &u8, context: &NumberContext) -> garde::Result {
    if value < &context.min {
      return Err(garde::Error::new("Number is too low"));
    }
    Ok(())
  }

  async fn test_handler(_query: Path<PathData>) -> HttpResponse {
    HttpResponse::Ok().finish()
  }

  async fn test_handler_with_context(_query: Path<PathDataWithContext>) -> HttpResponse {
    HttpResponse::Ok().finish()
  }

  #[tokio::test]
  async fn test_simple_path_validation() {
    let app = init_service(App::new().service(resource("/{age}/").route(post().to(test_handler)))).await;

    let req = TestRequest::post().uri("/24/").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::post().uri("/30/").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
  }

  #[tokio::test]
  async fn test_path_validation_custom_config() {
    let app = init_service(
      App::new()
        .app_data(
          PathConfig::default()
            .error_handler(|err, _req| InternalError::from_response(err, HttpResponse::Conflict().finish()).into()),
        )
        .service(resource("/{age}/").route(post().to(test_handler))),
    )
    .await;

    let req = TestRequest::post().uri("/24/").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::post().uri("/30/").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
  }

  #[tokio::test]
  async fn test_path_validation_with_context() {
    let number_context = NumberContext { min: 25 };
    let app = init_service(
      App::new()
        .app_data(number_context)
        .service(resource("/{age}/").route(post().to(test_handler_with_context))),
    )
    .await;

    let req = TestRequest::post().uri("/24/").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let req = TestRequest::post().uri("/30/").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
  }

  #[tokio::test]
  async fn test_path_validation_with_missing_context() {
    let app = init_service(App::new().service(resource("/{age}/").route(post().to(test_handler_with_context)))).await;

    let req = TestRequest::post().uri("/24/").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::post().uri("/30/").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
  }
}
