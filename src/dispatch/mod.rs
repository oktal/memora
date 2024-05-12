//! A module that contains components to dispatch commands to specific handlers

use std::{collections::HashMap, convert::Infallible, marker::PhantomData, pin::Pin, task::Poll};

use anyhow::{anyhow, bail};
use futures::{future::BoxFuture, Future, FutureExt};
use thiserror::Error;
use tower::{util::BoxService, Service};

use crate::resp;

#[derive(Debug, Error)]
pub enum Error {}

#[derive(Clone)]
pub struct Command {
    name: String,
    args: Vec<resp::Value>,
}

impl TryFrom<resp::Value> for Command {
    type Error = anyhow::Error;

    fn try_from(value: resp::Value) -> Result<Self, Self::Error> {
        // A command should be an array of resp values
        let resp::Value::Array(values) = value else {
            bail!("a valid command should be a RESP array");
        };

        let mut values = values.into_iter();

        // Attempt to retrieve the command name from the first element
        let name = values
            .next()
            .ok_or_else(|| anyhow!("expected a command name, but got an empty array"))?;

        // Command name should be a string
        let name = name
            .into_string()
            .ok_or_else(|| anyhow!("expected a string for the command"))?;

        // Arguments are the rest
        let args = values.collect();

        Ok(Self { name, args })
    }
}

pub trait CommandHandlerRegistry:
    Service<
    Command,
    Response = resp::Value,
    Error = Infallible,
    Future = BoxFuture<'static, Result<resp::Value, Infallible>>,
>
{
    fn handled_commands(&self) -> &[&'static str];
}

pub trait IntoValue {
    fn into_value(self) -> resp::Value;
}

impl<E> IntoValue for E
where
    E: AsRef<dyn std::error::Error>,
{
    fn into_value(self) -> resp::Value {
        resp::Value::error(self.as_ref().to_string())
    }
}

struct Blah;

impl IntoValue for Blah {
    fn into_value(self) -> resp::Value {
        resp::Value::simple("lol")
    }
}

impl IntoValue for resp::Value {
    fn into_value(self) -> resp::Value {
        self
    }
}

pub trait IntoHandlerService<S> {
    fn name(&self) -> &'static str;

    fn into_service(self, state: S) -> BoxService<Command, resp::Value, Infallible>;
}

struct MakeHandlerService<C, H> {
    handler: H,
    name: &'static str,
    _phantom: PhantomData<C>,
}

impl<C, S, H> IntoHandlerService<S> for MakeHandlerService<C, H>
where
    H: CommandHandler<C, S>,
    C: 'static,
    S: Clone + Send + 'static,
{
    fn name(&self) -> &'static str {
        self.name
    }

    fn into_service(self, state: S) -> BoxService<Command, resp::Value, Infallible> {
        BoxService::new(CommandHandlerService {
            handler: self.handler,
            state,
            _phantom: PhantomData,
        })
    }
}

pub struct CommandHandlerInvoker<S> {
    state: S,
    invokers: HashMap<&'static str, Vec<BoxService<Command, resp::Value, Infallible>>>,
}

impl<S> CommandHandlerInvoker<S>
where
    S: Clone + Send + 'static,
{
    pub fn with_state(state: S) -> Self {
        Self {
            state,
            invokers: HashMap::new(),
        }
    }

    pub fn handles<H>(&mut self, svc: H) -> &mut Self
    where
        H: IntoHandlerService<S>,
    {
        let name = svc.name();
        self.invokers
            .entry(name)
            .or_default()
            .push(svc.into_service(self.state.clone()));
        self
    }

    async fn call(&mut self, cmd: Command) -> Vec<resp::Value> {
        let mut responses = Vec::new();

        if let Some(invokers) = self.invokers.get_mut(cmd.name.as_str()) {
            for invoker in invokers {
                let res = invoker
                    .call(cmd.clone())
                    .await
                    .expect("calling a handler service is infaillible");
                responses.push(res);
            }
        }

        responses
    }
}

pub trait CommandHandler<C, S>: Copy + Clone + Send + 'static {
    type Future: Future<Output = resp::Value> + Send + 'static;

    fn handle(self, cmd: Command, state: S) -> Self::Future;

    fn into_service(self, name: &'static str) -> MakeHandlerService<C, Self> {
        MakeHandlerService {
            handler: self,
            name,
            _phantom: PhantomData,
        }
    }
}

struct CommandHandlerService<C, S, H, Fut> {
    handler: H,
    state: S,
    _phantom: PhantomData<fn(C) -> Fut>,
}

impl<C, S, H, Fut> Service<Command> for CommandHandlerService<C, S, H, Fut>
where
    H: CommandHandler<C, S, Future = Fut>,
    S: Clone + Send + 'static,
    Fut: Future<Output = resp::Value> + Send + 'static,
{
    type Error = Infallible;
    type Response = resp::Value;

    type Future = futures::future::Map<Fut, fn(resp::Value) -> Result<resp::Value, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Command) -> Self::Future {
        let res = CommandHandler::handle(self.handler, req, self.state.clone());
        let res = res.map(Ok as _);
        res
    }
}

pub struct CommandHandlerFuture<Fut> {
    fut: Fut,
}

impl<Fut, R> Future for CommandHandlerFuture<Fut>
where
    Fut: Future<Output = R> + Send + 'static,
    R: Into<resp::Value>,
{
    type Output = resp::Value;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        // First pin the future so that we can poll it
        // SAFETY: we just project our inner future to its own `Pin`
        // This is safe because we are already behind a Pin
        let fut = unsafe {
            let Self { fut } = self.get_unchecked_mut();
            let fut = Pin::new_unchecked(fut);
            fut
        };

        // Poll the future
        match fut.poll(cx) {
            // Future is ready, map the end result
            Poll::Ready(r) => Poll::Ready(r.into()),

            // Future is still pending
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<C, S, E, F, Fut, R> CommandHandler<C, S> for F
where
    F: FnOnce(C, S) -> Fut + Copy + Clone + Send + 'static,
    C: TryFrom<Vec<resp::Value>, Error = E>,
    E: IntoValue,
    Fut: Future<Output = R> + Send + 'static,
    R: Into<resp::Value>,
{
    type Future =
        futures::future::Either<CommandHandlerFuture<Fut>, futures::future::Ready<resp::Value>>;

    fn handle(self, cmd: Command, state: S) -> Self::Future {
        // First try to convert the create the typed commands from the list of arguments
        match C::try_from(cmd.args) {
            Ok(cmd) => {
                // Now call the function
                let fut = self(cmd, state);
                CommandHandlerFuture { fut }.left_future()
            }

            Err(e) => {
                // Conversion failed, return a ready future with the error message represented as a RESP value
                futures::future::ready(e.into_value()).right_future()
            }
        }
    }
}

#[cfg(test)]
mod test {

    use super::*;
    use crate::resp::{self, Value};

    struct Echo(String);

    impl TryFrom<Vec<resp::Value>> for Echo {
        type Error = anyhow::Error;

        fn try_from(value: Vec<resp::Value>) -> Result<Self, Self::Error> {
            let mut args = value.into_iter();
            let msg = args
                .next()
                .and_then(Value::into_string)
                .ok_or_else(|| anyhow!("`Echo` command requires a argument"))?;

            Ok(Self(msg))
        }
    }

    async fn echo(msg: Echo, _state: ()) -> resp::Value {
        resp::Value::simple(msg.0)
    }

    #[tokio::test]
    async fn should_call() {
        let mut registry = CommandHandlerInvoker::with_state(());
        registry.handles(echo.into_service("echo"));

        let responses = registry
            .call(Command {
                name: "echo".into(),
                args: vec![resp::Value::simple("test")],
            })
            .await;

        assert_eq!(responses, vec![resp::Value::simple("test")]);
    }
}
