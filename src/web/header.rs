use crate::validate_for_request;
use actix_http::header::Header as ParseHeader;
use actix_web::dev::Payload;
use actix_web::error::Error;
use actix_web::{FromRequest, HttpRequest};
use derive_more::{AsRef, Deref, DerefMut, Display, From};
use futures::future::{err, ok, Ready};
use garde::Validate;

/// Drop in replacement for [actix_web::web::Header](https://docs.rs/actix-web/latest/actix_web/web/struct.Header.html)
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Deref, DerefMut, AsRef, Display, From)]
pub struct Header<T>(pub T);

impl<T> Header<T> {
  /// Unwrap into the inner `T` value.
  pub fn into_inner(self) -> T {
    self.0
  }
}

impl<T> FromRequest for Header<T>
where
  T: ParseHeader + Validate + 'static,
  T::Context: Default,
{
  type Error = Error;
  type Future = Ready<Result<Self, Self::Error>>;

  #[inline]
  fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
    match ParseHeader::parse(req) {
      Ok(header) => match validate_for_request(header, &req.clone()) {
        Ok(header) => ok(Header(header)),
        Err(e) => err(e.into()),
      },
      Err(e) => err(e.into()),
    }
  }
}

#[cfg(test)]
mod test {
  use crate::web::Header;
  use actix_http::error::ParseError;
  use actix_http::header::Header as ParseHeader;
  use actix_http::header::{HeaderName, HeaderValue, InvalidHeaderValue, TryIntoHeaderValue};
  use actix_http::{HttpMessage, StatusCode};
  use actix_test::TestRequest;
  use actix_web::FromRequest;
  use garde::Validate;
  use serde::{Deserialize, Serialize};

  #[derive(Debug, PartialEq, Validate, Serialize, Deserialize)]
  struct HeaderData {
    #[garde(range(min = 18, max = 28))]
    age: u8,
  }

  impl TryIntoHeaderValue for HeaderData {
    type Error = InvalidHeaderValue;

    fn try_into_value(self) -> Result<HeaderValue, Self::Error> {
      HeaderValue::try_from(self.age.to_string())
    }
  }

  impl ParseHeader for HeaderData {
    fn name() -> HeaderName {
      HeaderName::from_static("header-data")
    }

    fn parse<M: HttpMessage>(msg: &M) -> Result<Self, ParseError> {
      msg
        .headers()
        .get(&Self::name())
        .ok_or_else(|| ParseError::Header)
        .and_then(|v| v.to_str().map_err(|_| ParseError::Header))
        .and_then(|v| v.parse::<u8>().map_err(|_| ParseError::Header))
        .map(|v| HeaderData { age: v })
    }
  }

  #[tokio::test]
  async fn test_simple_header_validation() {
    let (req, mut pl) = TestRequest::default()
      .insert_header(("header-data", HeaderData { age: 10 }))
      .to_http_parts();

    #[allow(clippy::unwrap_used)]
    let res = Header::<HeaderData>::from_request(&req, &mut pl).await.unwrap_err();
    assert_eq!(res.as_response_error().status_code(), StatusCode::BAD_REQUEST);

    let (req, mut pl) = TestRequest::default()
      .insert_header(("header-data", HeaderData { age: 25 }))
      .to_http_parts();

    let res = Header::<HeaderData>::from_request(&req, &mut pl).await;
    assert!(res.is_ok());
  }
}
