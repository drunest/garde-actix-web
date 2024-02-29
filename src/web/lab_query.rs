use crate::validate_for_request;
use actix_web::dev::Payload;
use actix_web::error::QueryPayloadError;
use actix_web::{Error, FromRequest, HttpRequest};
use actix_web_lab::__reexports::futures_util::future::LocalBoxFuture;
use garde::Validate;
use serde::de;
use serde::de::DeserializeOwned;
use crate::web::QueryConfig;

/// Drop in replacement for [actix_web_lab::extract::Query](https://docs.rs/actix-web-lab/latest/actix_web_lab/extract/struct.Query.html)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Query<T>(pub T);

impl<T> Query<T> {
  pub fn into_inner(self) -> T {
    self.0
  }
}

impl<T: DeserializeOwned> Query<T> {
  pub fn from_query(query_str: &str) -> Result<Self, QueryPayloadError> {
    actix_web_lab::extract::Query::from_query(query_str)
      .map(|r: actix_web_lab::extract::Query<T>| Self(r.into_inner()))
  }
}

impl<T> FromRequest for Query<T>
  where
    T: DeserializeOwned + Validate + 'static,
    T::Context: Default,
{
  type Error = Error;
  type Future = LocalBoxFuture<'static, Result<Self, Self::Error>>;

  #[inline]
  fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
    let req_copy = req.clone();
    let req = req.clone();
    let mut payload = payload.take();

    let error_handler = req.app_data::<QueryConfig>().and_then(|c| c.err_handler.clone());

    Box::pin(async move {
      actix_web_lab::extract::Query::from_request(&req, &mut payload)
        .await
        .map_err(|e| QueryPayloadError::Deserialize(de::Error::custom(format!("{}", e))).into())
        .and_then(|data| {
          let req = req_copy;
          validate_for_request(data.0, &req)
        })
        .map(|res| Self(res))
        .or_else(move |e| {
          log::debug!(
          "Failed during Query extractor deserialization. \
                     Request path: {:?}",
          req.path()
        );

          let e = if let Some(error_handler) = error_handler {
            (error_handler)(e, &req)
          } else {
            e.into()
          };

          Err(e)
        })
    })
  }
}

#[cfg(test)]
mod test {
  use crate::web::{Query, QueryConfig};
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

  async fn test_handler(_query: Query<QueryData>) -> HttpResponse {
    HttpResponse::Ok().finish()
  }

  async fn test_handler_with_context(_query: Query<QueryDataWithContext>) -> HttpResponse {
    HttpResponse::Ok().finish()
  }

  #[tokio::test]
  async fn test_simple_lab_query_validation() {
    let app = init_service(App::new().service(resource("/").route(post().to(test_handler)))).await;

    let req = TestRequest::post().uri("/?age=24").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::post().uri("/?age=30").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
  }

  #[tokio::test]
  async fn test_lab_query_validation_custom_config() {
    let app = init_service(
      App::new()
        .app_data(
          QueryConfig::default()
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
  async fn test_lab_query_validation_with_context() {
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
  async fn test_lab_query_validation_with_missing_context() {
    let app = init_service(App::new().service(resource("/").route(post().to(test_handler_with_context)))).await;

    let req = TestRequest::post().uri("/?age=24").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::post().uri("/?age=30").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
  }
}
