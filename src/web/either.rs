use super::form::Form;
use super::json::Json;
use actix_web::dev::Payload;
use actix_web::web::Bytes;
use actix_web::{Error, FromRequest, HttpRequest};
use futures::ready;
use pin_project_lite::pin_project;
use std::future::Future;
use std::mem;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

/// Drop in replacement for [actix_web::web::Either](https://docs.rs/actix-web/latest/actix_web/web/enum.Either.html)
#[derive(Debug, PartialEq, Eq)]
pub enum Either<L, R> {
  Left(L),
  Right(R),
}

impl<T> Either<Form<T>, Json<T>> {
  pub fn into_inner(self) -> T {
    match self {
      Either::Left(form) => form.into_inner(),
      Either::Right(json) => json.into_inner(),
    }
  }
}

impl<T> Either<Json<T>, Form<T>> {
  pub fn into_inner(self) -> T {
    match self {
      Either::Left(json) => json.into_inner(),
      Either::Right(form) => form.into_inner(),
    }
  }
}

#[derive(Debug)]
pub enum EitherExtractError<L, R> {
  Bytes(Error),
  Extract(L, R),
}

impl<L, R> From<EitherExtractError<L, R>> for Error
where
  L: Into<Error>,
  R: Into<Error>,
{
  fn from(err: EitherExtractError<L, R>) -> Error {
    match err {
      EitherExtractError::Bytes(err) => err,
      EitherExtractError::Extract(a_err, _b_err) => a_err.into(),
    }
  }
}

impl<L, R> FromRequest for Either<L, R>
where
  L: FromRequest + 'static,
  R: FromRequest + 'static,
{
  type Error = EitherExtractError<L::Error, R::Error>;
  type Future = EitherExtractFut<L, R>;

  fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
    EitherExtractFut {
      req: req.clone(),
      state: EitherExtractState::Bytes {
        bytes: Bytes::from_request(req, payload),
      },
    }
  }
}

pin_project! {
    pub struct EitherExtractFut<L, R>
    where
        R: FromRequest,
        L: FromRequest,
    {
        req: HttpRequest,
        #[pin]
        state: EitherExtractState<L, R>,
    }
}

pin_project! {
    #[project = EitherExtractProj]
    pub enum EitherExtractState<L, R>
    where
        L: FromRequest,
        R: FromRequest,
    {
        Bytes {
            #[pin]
            bytes: <Bytes as FromRequest>::Future,
        },
        Left {
            #[pin]
            left: L::Future,
            fallback: Bytes,
        },
        Right {
            #[pin]
            right: R::Future,
            left_err: Option<L::Error>,
        },
    }
}

impl<R, RF, RE, L, LF, LE> Future for EitherExtractFut<L, R>
where
  L: FromRequest<Future = LF, Error = LE>,
  R: FromRequest<Future = RF, Error = RE>,
  LF: Future<Output = Result<L, LE>> + 'static,
  RF: Future<Output = Result<R, RE>> + 'static,
  LE: Into<Error>,
  RE: Into<Error>,
{
  type Output = Result<Either<L, R>, EitherExtractError<LE, RE>>;

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let mut this = self.project();
    let ready = loop {
      let next = match this.state.as_mut().project() {
        EitherExtractProj::Bytes { bytes } => {
          let res = ready!(bytes.poll(cx));
          match res {
            Ok(bytes) => {
              let fallback = bytes.clone();
              let left = L::from_request(this.req, &mut payload_from_bytes(bytes));
              EitherExtractState::Left { left, fallback }
            }
            Err(err) => break Err(EitherExtractError::Bytes(err)),
          }
        }
        EitherExtractProj::Left { left, fallback } => {
          let res = ready!(left.poll(cx));
          match res {
            Ok(extracted) => break Ok(Either::Left(extracted)),
            Err(left_err) => {
              let right = R::from_request(this.req, &mut payload_from_bytes(mem::take(fallback)));
              EitherExtractState::Right {
                left_err: Some(left_err),
                right,
              }
            }
          }
        }
        EitherExtractProj::Right { right, left_err } => {
          let res = ready!(right.poll(cx));
          match res {
            Ok(data) => break Ok(Either::Right(data)),
            Err(err) => {
              #[allow(clippy::unwrap_used)]
              break Err(EitherExtractError::Extract(left_err.take().unwrap(), err));
            }
          }
        }
      };
      this.state.set(next);
    };

    Poll::Ready(ready)
  }
}

fn payload_from_bytes(bytes: Bytes) -> Payload {
  let (_, mut h1_payload) = actix_http::h1::Payload::create(true);
  h1_payload.unread_data(bytes);
  Payload::from(h1_payload)
}
