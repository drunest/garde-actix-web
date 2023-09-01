use crate::validate_for_request;
use actix_web::dev::{JsonBody, Payload};
use actix_web::{web, Error, FromRequest, HttpRequest};
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use garde::Validate;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::{fmt, ops};

/// Drop in replacement for [actix_web::web::Json](https://docs.rs/actix-web/latest/actix_web/web/struct.Json.html)
#[derive(Debug)]
pub struct Json<T>(pub T);

impl<T> Json<T> {
  pub fn into_inner(self) -> T {
    self.0
  }
}

impl<T> ops::Deref for Json<T> {
  type Target = T;

  fn deref(&self) -> &T {
    &self.0
  }
}

impl<T> ops::DerefMut for Json<T> {
  fn deref_mut(&mut self) -> &mut T {
    &mut self.0
  }
}

impl<T: fmt::Display> fmt::Display for Json<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Display::fmt(&self.0, f)
  }
}

impl<T> FromRequest for Json<T>
where
  T: DeserializeOwned + Validate + 'static,
  T::Context: Default,
{
  type Error = Error;
  type Future = LocalBoxFuture<'static, Result<Self, Self::Error>>;

  #[inline]
  fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
    let req_copy = req.clone();
    let req_copy2 = req.clone();

    let config = JsonConfig::from_req(req);

    let limit = config.limit;
    let ctype_required = config.content_type_required;
    let ctype_fn = config.content_type.as_deref();
    let err_handler = config.err_handler.clone();

    JsonBody::new(req, payload, ctype_fn, ctype_required)
      .limit(limit)
      .map(move |res: Result<T, _>| match res {
        Ok(data) => {
          let req = req_copy;
          validate_for_request(data, &req)
        }
        Err(e) => Err(e.into()),
      })
      .map(move |res| match res {
        Err(err) => {
          log::debug!(
            "Failed to deserialize Json from payload. \
                         Request path: {}",
            req_copy2.path()
          );

          if let Some(err_handler) = err_handler.as_ref() {
            Err((*err_handler)(err, &req_copy2))
          } else {
            Err(err.into())
          }
        }
        Ok(data) => Ok(Json(data)),
      })
      .boxed_local()
  }
}

type JsonErrorHandler = Option<Arc<dyn Fn(crate::error::Error, &HttpRequest) -> Error + Send + Sync>>;

/// Replacement for [actix_web::web::JsonConfig](https://docs.rs/actix-web/latest/actix_web/web/struct.JsonConfig.html)
/// Error handler must map from an `garde_actix_web::error::Error`
#[derive(Clone)]
pub struct JsonConfig {
  limit: usize,
  err_handler: JsonErrorHandler,
  content_type: Option<Arc<dyn Fn(mime::Mime) -> bool + Send + Sync>>,
  content_type_required: bool,
}

impl JsonConfig {
  pub fn limit(mut self, limit: usize) -> Self {
    self.limit = limit;
    self
  }

  pub fn error_handler<F>(mut self, f: F) -> Self
  where
    F: Fn(crate::error::Error, &HttpRequest) -> Error + Send + Sync + 'static,
  {
    self.err_handler = Some(Arc::new(f));
    self
  }

  pub fn content_type<F>(mut self, predicate: F) -> Self
  where
    F: Fn(mime::Mime) -> bool + Send + Sync + 'static,
  {
    self.content_type = Some(Arc::new(predicate));
    self
  }

  pub fn content_type_required(mut self, content_type_required: bool) -> Self {
    self.content_type_required = content_type_required;
    self
  }

  pub fn from_req(req: &HttpRequest) -> &Self {
    req
      .app_data::<Self>()
      .or_else(|| req.app_data::<web::Data<Self>>().map(|d| d.as_ref()))
      .unwrap_or(&DEFAULT_CONFIG)
  }
}

const DEFAULT_LIMIT: usize = 2_097_152; // 2 mb

const DEFAULT_CONFIG: JsonConfig = JsonConfig {
  limit: DEFAULT_LIMIT,
  err_handler: None,
  content_type: None,
  content_type_required: true,
};

impl Default for JsonConfig {
  fn default() -> Self {
    DEFAULT_CONFIG
  }
}

#[cfg(test)]
mod test {
  use crate::web::{Json, JsonConfig};
  use actix_http::StatusCode;
  use actix_web::error::InternalError;
  use actix_web::test::{call_service, init_service, TestRequest};
  use actix_web::web::{post, resource};
  use actix_web::{App, HttpResponse};
  use garde::Validate;
  use serde::{Deserialize, Serialize};

  #[derive(Debug, PartialEq, Validate, Serialize, Deserialize)]
  struct JsonData {
    #[garde(range(min = 18, max = 28))]
    age: u8,
  }

  #[derive(Debug, PartialEq, Validate, Serialize, Deserialize)]
  #[garde(context(NumberContext))]
  struct JsonDataWithContext {
    #[garde(custom(is_big_enough))]
    age: u8,
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

  async fn test_handler(_query: Json<JsonData>) -> HttpResponse {
    HttpResponse::Ok().finish()
  }

  async fn test_handler_with_context(_query: Json<JsonDataWithContext>) -> HttpResponse {
    HttpResponse::Ok().finish()
  }

  #[tokio::test]
  async fn test_simple_json_validation() {
    let app = init_service(App::new().service(resource("/").route(post().to(test_handler)))).await;

    let req = TestRequest::post()
      .uri("/")
      .set_json(&JsonData { age: 24 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::post()
      .uri("/")
      .set_json(&JsonData { age: 30 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
  }

  #[tokio::test]
  async fn test_json_validation_custom_config() {
    let app = init_service(
      App::new()
        .app_data(
          JsonConfig::default()
            .error_handler(|err, _req| InternalError::from_response(err, HttpResponse::Conflict().finish()).into()),
        )
        .service(resource("/").route(post().to(test_handler))),
    )
    .await;

    let req = TestRequest::post()
      .uri("/")
      .set_json(&JsonData { age: 24 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::post()
      .uri("/")
      .set_json(&JsonData { age: 30 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
  }

  #[tokio::test]
  async fn test_json_validation_with_context() {
    let number_context = NumberContext { min: 25 };
    let app = init_service(
      App::new()
        .app_data(number_context)
        .service(resource("/").route(post().to(test_handler_with_context))),
    )
    .await;

    let req = TestRequest::post()
      .uri("/")
      .set_json(&JsonData { age: 24 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let req = TestRequest::post()
      .uri("/")
      .set_json(&JsonData { age: 30 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
  }

  #[tokio::test]
  async fn test_json_validation_with_missing_context() {
    let app = init_service(App::new().service(resource("/").route(post().to(test_handler_with_context)))).await;

    let req = TestRequest::post()
      .uri("/")
      .set_json(&JsonData { age: 24 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::post()
      .uri("/")
      .set_json(&JsonData { age: 30 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
  }
}
