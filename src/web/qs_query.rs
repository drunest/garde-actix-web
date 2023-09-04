use crate::validate_for_request;
use actix_web::dev::Payload;
use actix_web::error::QueryPayloadError;
use actix_web::{Error, FromRequest, HttpRequest};
use derive_more::{AsRef, Deref, DerefMut, Display, From};
use futures::future::{err, ok, Ready};
use garde::Validate;
use serde::de::DeserializeOwned;
use serde_qs::Config;
use std::sync::Arc;

/// Drop in replacement for [serde_qs::actix::QsQuery](https://docs.rs/serde_qs/latest/serde_qs/actix/struct.QsQuery.html)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deref, DerefMut, AsRef, Display, From)]
pub struct QsQuery<T>(pub T);

impl<T> QsQuery<T> {
  pub fn into_inner(self) -> T {
    self.0
  }
}

impl<T: DeserializeOwned> QsQuery<T> {
  pub fn from_query(query_str: &str) -> Result<Self, QueryPayloadError> {
    serde_urlencoded::from_str::<T>(query_str)
      .map(Self)
      .map_err(QueryPayloadError::Deserialize)
  }
}

impl<T> FromRequest for QsQuery<T>
where
  T: DeserializeOwned + Validate + 'static,
  T::Context: Default,
{
  type Error = Error;
  type Future = Ready<Result<Self, Error>>;

  #[inline]
  fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
    let req_copy = req.clone();
    let qs_query_config = req.app_data::<QsQueryConfig>();
    let error_handler = qs_query_config.and_then(|c| c.err_handler.clone());
    let default_qs_config = Config::default();
    let qs_config = qs_query_config
      .map(|config| &config.qs_config)
      .unwrap_or(&default_qs_config);

    qs_config
      .deserialize_str::<T>(req.query_string())
      .map_err(Into::into)
      .and_then(|data: T| {
        let req = req_copy;
        validate_for_request(data, &req)
      })
      .map(|val| ok(QsQuery(val)))
      .unwrap_or_else(move |e| {
        log::debug!(
          "Failed during Query extractor deserialization. \
                     Request path: {:?}",
          req.path()
        );

        let e = if let Some(error_handler) = error_handler {
          (error_handler)(e, req)
        } else {
          e.into()
        };

        err(e)
      })
  }
}

/// Replacement for [serde_qs::actix::QsQueryConfig](https://docs.rs/serde_qs/latest/serde_qs/actix/struct.QsQueryConfig.html)
/// Error handler must map from an `garde_actix_web::error::Error`
#[derive(Default)]
pub struct QsQueryConfig {
  #[allow(clippy::type_complexity)]
  err_handler: Option<Arc<dyn Fn(crate::error::Error, &HttpRequest) -> Error + Send + Sync>>,
  qs_config: Config,
}

impl QsQueryConfig {
  pub fn error_handler<F>(mut self, f: F) -> Self
  where
    F: Fn(crate::error::Error, &HttpRequest) -> Error + Send + Sync + 'static,
  {
    self.err_handler = Some(Arc::new(f));
    self
  }

  pub fn qs_config(mut self, config: Config) -> Self {
    self.qs_config = config;
    self
  }
}

#[cfg(test)]
mod test {
  use crate::web::{QsQuery, QsQueryConfig};
  use actix_http::StatusCode;
  use actix_web::error::InternalError;
  use actix_web::test::{call_service, init_service, TestRequest};
  use actix_web::web::{post, resource};
  use actix_web::{App, HttpResponse};
  use garde::Validate;
  use serde::{Deserialize, Serialize};

  #[derive(Debug, PartialEq, Validate, Serialize, Deserialize)]
  struct QueryData {
    #[garde(range(min = 18, max = 28))]
    age: u8,
  }

  #[derive(Debug, PartialEq, Validate, Serialize, Deserialize)]
  #[garde(context(NumberContext))]
  struct QueryDataWithContext {
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

  async fn test_handler(_query: QsQuery<QueryData>) -> HttpResponse {
    HttpResponse::Ok().finish()
  }

  async fn test_handler_with_context(_query: QsQuery<QueryDataWithContext>) -> HttpResponse {
    HttpResponse::Ok().finish()
  }

  #[tokio::test]
  async fn test_simple_query_validation() {
    let app = init_service(App::new().service(resource("/").route(post().to(test_handler)))).await;

    let req = TestRequest::post().uri("/?age=24").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::post().uri("/?age=30").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
  }

  #[tokio::test]
  async fn test_query_validation_custom_config() {
    let app = init_service(
      App::new()
        .app_data(
          QsQueryConfig::default()
            .error_handler(|err, _req| InternalError::from_response(err, HttpResponse::Conflict().finish()).into()),
        )
        .service(resource("/").route(post().to(test_handler))),
    )
    .await;

    let req = TestRequest::post().uri("/?age=24").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::post().uri("/?age=30").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
  }

  #[tokio::test]
  async fn test_query_validation_with_context() {
    let number_context = NumberContext { min: 25 };
    let app = init_service(
      App::new()
        .app_data(number_context)
        .service(resource("/").route(post().to(test_handler_with_context))),
    )
    .await;

    let req = TestRequest::post().uri("/?age=24").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let req = TestRequest::post().uri("/?age=30").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
  }

  #[tokio::test]
  async fn test_query_validation_with_missing_context() {
    let app = init_service(App::new().service(resource("/").route(post().to(test_handler_with_context)))).await;

    let req = TestRequest::post().uri("/?age=24").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::post().uri("/?age=30").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
  }
}
