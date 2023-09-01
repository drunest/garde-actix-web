use actix_http::Payload;
use actix_web::web::UrlEncoded;
use actix_web::{web, Error, FromRequest, HttpRequest};
use serde::{de::DeserializeOwned, Serialize};
use std::rc::Rc;

use crate::validate_for_request;
use derive_more::{AsRef, Deref, DerefMut, Display, From};
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use garde::Validate;

/// Drop in replacement for [actix_web::web::Form](https://docs.rs/actix-web/latest/actix_web/web/struct.Form.html)
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Deref, DerefMut, AsRef, Display, From)]
pub struct Form<T>(pub T);

impl<T> Form<T> {
  pub fn into_inner(self) -> T {
    self.0
  }
}

impl<T> Serialize for Form<T>
where
  T: Serialize,
{
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    self.0.serialize(serializer)
  }
}

impl<T> FromRequest for Form<T>
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

    let FormConfig { limit, err_handler } = FormConfig::from_req(req).clone();

    UrlEncoded::new(req, payload)
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
          if let Some(err_handler) = err_handler.as_ref() {
            Err((*err_handler)(err, &req_copy2))
          } else {
            Err(err.into())
          }
        }
        Ok(data) => Ok(Form(data)),
      })
      .boxed_local()
  }
}

type FormErrHandler = Option<Rc<dyn Fn(crate::error::Error, &HttpRequest) -> Error>>;

/// Replacement for [actix_web::web::FormConfig](https://docs.rs/actix-web/latest/actix_web/web/struct.FormConfig.html)
/// Error handler must map from an `actix_web_garde::error::Error`
#[derive(Clone)]
pub struct FormConfig {
  limit: usize,
  err_handler: FormErrHandler,
}

impl FormConfig {
  pub fn limit(mut self, limit: usize) -> Self {
    self.limit = limit;
    self
  }

  pub fn error_handler<F>(mut self, f: F) -> Self
  where
    F: Fn(crate::error::Error, &HttpRequest) -> Error + 'static,
  {
    self.err_handler = Some(Rc::new(f));
    self
  }

  fn from_req(req: &HttpRequest) -> &Self {
    req
      .app_data::<Self>()
      .or_else(|| req.app_data::<web::Data<Self>>().map(|d| d.as_ref()))
      .unwrap_or(&DEFAULT_CONFIG)
  }
}

const DEFAULT_CONFIG: FormConfig = FormConfig {
  limit: 16_384, // 2^14 bytes (~16kB)
  err_handler: None,
};

impl Default for FormConfig {
  fn default() -> Self {
    DEFAULT_CONFIG
  }
}

#[cfg(test)]
mod test {
  use crate::web::{Form, FormConfig};
  use actix_http::StatusCode;
  use actix_web::error::InternalError;
  use actix_web::test::{call_service, init_service, TestRequest};
  use actix_web::web::{post, resource};
  use actix_web::{App, HttpResponse};
  use garde::Validate;
  use serde::{Deserialize, Serialize};

  #[derive(Debug, PartialEq, Validate, Serialize, Deserialize)]
  struct FormData {
    #[garde(range(min = 18, max = 28))]
    age: u8,
  }

  #[derive(Debug, PartialEq, Validate, Serialize, Deserialize)]
  #[garde(context(NumberContext))]
  struct FormDataWithContext {
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

  async fn test_handler(_query: Form<FormData>) -> HttpResponse {
    HttpResponse::Ok().finish()
  }

  async fn test_handler_with_context(_query: Form<FormDataWithContext>) -> HttpResponse {
    HttpResponse::Ok().finish()
  }

  #[tokio::test]
  async fn test_simple_form_validation() {
    let app = init_service(App::new().service(resource("/").route(post().to(test_handler)))).await;

    let req = TestRequest::post()
      .uri("/")
      .set_form(&FormData { age: 24 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::post()
      .uri("/")
      .set_form(&FormData { age: 30 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
  }

  #[tokio::test]
  async fn test_form_validation_custom_config() {
    let app = init_service(
      App::new()
        .app_data(
          FormConfig::default()
            .error_handler(|err, _req| InternalError::from_response(err, HttpResponse::Conflict().finish()).into()),
        )
        .service(resource("/").route(post().to(test_handler))),
    )
    .await;

    let req = TestRequest::post()
      .uri("/")
      .set_form(&FormData { age: 24 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::post()
      .uri("/")
      .set_form(&FormData { age: 30 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
  }

  #[tokio::test]
  async fn test_form_validation_with_context() {
    let number_context = NumberContext { min: 25 };
    let app = init_service(
      App::new()
        .app_data(number_context)
        .service(resource("/").route(post().to(test_handler_with_context))),
    )
    .await;

    let req = TestRequest::post()
      .uri("/")
      .set_form(&FormData { age: 24 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let req = TestRequest::post()
      .uri("/")
      .set_form(&FormData { age: 30 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
  }

  #[tokio::test]
  async fn test_form_validation_with_missing_context() {
    let app = init_service(App::new().service(resource("/").route(post().to(test_handler_with_context)))).await;

    let req = TestRequest::post()
      .uri("/")
      .set_form(&FormData { age: 24 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let req = TestRequest::post()
      .uri("/")
      .set_form(&FormData { age: 30 })
      .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
  }
}
