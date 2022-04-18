use super::{
    builder::ShardBuilder,
    command::Command,
    config::Config,
    emitter::Emitter,
    event::Events,
    json,
    processor::{ConnectingErrorType, Latency, Session, ShardProcessor},
    raw_message::Message,
    stage::Stage,
};
use crate::Intents;
use leaky_bucket_lite::LeakyBucket;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
    sync::{atomic::Ordering, Arc, Mutex},
};
use tokio::{
    sync::{watch::Receiver as WatchReceiver, OnceCell},
    task::JoinHandle,
    time::Instant,
};
use tokio_tungstenite::tungstenite::protocol::{
    frame::coding::CloseCode, CloseFrame as TungsteniteCloseFrame,
};

/// Sending a command failed.
#[derive(Debug)]
pub struct CommandError {
    kind: CommandErrorType,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl CommandError {
    /// Immutable reference to the type of error that occurred.
    #[must_use = "retrieving the type has no effect if left unused"]
    pub const fn kind(&self) -> &CommandErrorType {
        &self.kind
    }

    /// Consume the error, returning the source error if there is any.
    #[must_use = "consuming the error and retrieving the source has no effect if left unused"]
    pub fn into_source(self) -> Option<Box<dyn Error + Send + Sync>> {
        self.source
    }

    /// Consume the error, returning the owned error type and the source error.
    #[must_use = "consuming the error into its parts has no effect if left unused"]
    pub fn into_parts(self) -> (CommandErrorType, Option<Box<dyn Error + Send + Sync>>) {
        (self.kind, None)
    }
}

impl CommandError {
    pub(crate) fn from_send(error: SendError) -> Self {
        let (kind, source) = error.into_parts();

        let new_kind = match kind {
            SendErrorType::HeartbeaterNotStarted => CommandErrorType::HeartbeaterNotStarted,
            SendErrorType::Sending => CommandErrorType::Sending,
            SendErrorType::SessionInactive => CommandErrorType::SessionInactive,
        };

        Self {
            kind: new_kind,
            source,
        }
    }
}

impl Display for CommandError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match &self.kind {
            CommandErrorType::HeartbeaterNotStarted => {
                f.write_str("heartbeater task hasn't been started yet")
            }
            CommandErrorType::Sending => {
                f.write_str("sending the message over the websocket failed")
            }
            CommandErrorType::Serializing => f.write_str("serializing the value as json failed"),
            CommandErrorType::SessionInactive => Display::fmt(&SessionInactiveError, f),
        }
    }
}

impl Error for CommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| &**source as &(dyn Error + 'static))
    }
}

/// Type of [`CommandError`] that occurred.
#[derive(Debug)]
#[non_exhaustive]
pub enum CommandErrorType {
    /// Heartbeater task has not been started yet.
    HeartbeaterNotStarted,
    /// Sending the payload over the WebSocket failed. This is indicative of a
    /// shutdown shard.
    Sending,
    /// Serializing the payload as JSON failed.
    Serializing,
    /// Shard's session is inactive because the shard hasn't been started.
    SessionInactive,
}

/// Shard's session is inactive.
///
/// This means that the shard has not yet been started.
#[derive(Debug)]
#[non_exhaustive]
pub struct SessionInactiveError;

impl Display for SessionInactiveError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str("the shard session is inactive and was not started")
    }
}

impl Error for SessionInactiveError {}

/// Starting a shard and connecting to the gateway failed.
#[derive(Debug)]
pub struct SendError {
    kind: SendErrorType,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl SendError {
    /// Immutable reference to the type of error that occurred.
    #[must_use = "retrieving the type has no effect if left unused"]
    pub const fn kind(&self) -> &SendErrorType {
        &self.kind
    }

    /// Consume the error, returning the source error if there is any.
    #[must_use = "consuming the error and retrieving the source has no effect if left unused"]
    pub fn into_source(self) -> Option<Box<dyn Error + Send + Sync>> {
        self.source
    }

    /// Consume the error, returning the owned error type and the source error.
    #[must_use = "consuming the error into its parts has no effect if left unused"]
    pub fn into_parts(self) -> (SendErrorType, Option<Box<dyn Error + Send + Sync>>) {
        (self.kind, None)
    }
}

impl Display for SendError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match &self.kind {
            SendErrorType::HeartbeaterNotStarted { .. } => {
                f.write_str("heartbeater task hasn't been started yet")
            }
            SendErrorType::Sending { .. } => {
                f.write_str("sending the message over the websocket failed")
            }
            SendErrorType::SessionInactive { .. } => f.write_str("shard hasn't been started"),
        }
    }
}

impl Error for SendError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| &**source as &(dyn Error + 'static))
    }
}

/// Type of [`SendError`] that occurred.
#[derive(Debug)]
#[non_exhaustive]
pub enum SendErrorType {
    /// Heartbeater task has not been started yet.
    HeartbeaterNotStarted,
    /// Sending the payload over the WebSocket failed. This is indicative of a
    /// shard that isn't properly running.
    Sending,
    /// Shard's session is inactive because the shard hasn't been started.
    SessionInactive,
}

/// Starting a shard and connecting to the gateway failed.
#[derive(Debug)]
pub struct ShardStartError {
    kind: ShardStartErrorType,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl ShardStartError {
    /// Immutable reference to the type of error that occurred.
    #[must_use = "retrieving the type has no effect if left unused"]
    pub const fn kind(&self) -> &ShardStartErrorType {
        &self.kind
    }

    /// Consume the error, returning the source error if there is any.
    #[must_use = "consuming the error and retrieving the source has no effect if left unused"]
    pub fn into_source(self) -> Option<Box<dyn Error + Send + Sync>> {
        self.source
    }

    /// Consume the error, returning the owned error type and the source error.
    #[must_use = "consuming the error into its parts has no effect if left unused"]
    pub fn into_parts(self) -> (ShardStartErrorType, Option<Box<dyn Error + Send + Sync>>) {
        (self.kind, None)
    }
}

impl Display for ShardStartError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match &self.kind {
            ShardStartErrorType::AlreadyStarted => {
                f.write_str("shard has already been previously started")
            }
            ShardStartErrorType::Establishing => f.write_str("establishing the connection failed"),
            ShardStartErrorType::ParsingGatewayUrl { url } => {
                f.write_str("the gateway url `")?;
                f.write_str(url)?;

                f.write_str("` is invalid")
            }
            ShardStartErrorType::RetrievingGatewayUrl => {
                f.write_str("retrieving the gateway URL via HTTP failed")
            }
        }
    }
}

impl Error for ShardStartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| &**source as &(dyn Error + 'static))
    }
}

/// Type of [`ShardStartError`] that occurred.
#[derive(Debug)]
#[non_exhaustive]
pub enum ShardStartErrorType {
    /// Shard has already been previously started.
    ///
    /// Shards can't be started multiple times; you need to create a new
    /// instance of the shard.
    AlreadyStarted,
    /// Establishing a connection to the gateway failed.
    Establishing,
    /// Parsing the gateway URL provided by Discord to connect to the gateway
    /// failed due to an invalid URL.
    ParsingGatewayUrl {
        /// URL that couldn't be parsed.
        url: String,
    },
    /// Retrieving the gateway URL via the Twilight HTTP client failed.
    RetrievingGatewayUrl,
}

/// Information about a shard, including its latency, current session sequence,
/// and connection stage.
#[derive(Clone, Debug)]
pub struct Information {
    id: u64,
    latency: Latency,
    ratelimit_refill: Option<Instant>,
    ratelimit_requests: Option<u32>,
    session_id: Option<Box<str>>,
    seq: u64,
    stage: Stage,
}

impl Information {
    /// Return the ID of the shard.
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Return an immutable reference to the latency information for the shard.
    ///
    /// This includes the average latency over all time, and the latency
    /// information for the 5 most recent heartbeats.
    pub const fn latency(&self) -> &Latency {
        &self.latency
    }

    /// When the ratelimiter will next refill the [`ratelimit_requests`].
    ///
    /// This will be `None` if payload ratelimiting has been disabled.
    ///
    /// [`ratelimit_requests`]: Self::ratelimit_requests
    pub const fn ratelimit_refill(&self) -> Option<Instant> {
        self.ratelimit_refill
    }

    /// Number of requests remaining until the next [`ratelimit_refill`].
    ///
    /// This will be `None` if payload ratelimiting has been disabled.
    ///
    /// [`ratelimit_refill`]: Self::ratelimit_refill
    pub const fn ratelimit_requests(&self) -> Option<u32> {
        self.ratelimit_requests
    }

    /// Return an immutable reference to the session ID of the shard.
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Current sequence of the connection.
    ///
    /// This is the number of the event that was received this session (without
    /// reconnecting). A larger number typically correlates that the shard has
    /// been connected for a longer time, while a smaller number typically
    /// correlates to meaning that it's been connected for a less amount of
    /// time.
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    /// Current stage of the shard.
    ///
    /// For example, once a shard is fully booted then it will be [`Connected`].
    ///
    /// [`Connected`]: Stage::Connected
    pub const fn stage(&self) -> Stage {
        self.stage
    }
}

/// Details to resume a gateway session.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResumeSession {
    /// ID of the session being resumed.
    pub session_id: String,
    /// Last received event sequence number.
    pub sequence: u64,
}

/// Shard to run and manage a session with the gateway.
///
/// Shards are responsible for handling incoming events, process events relevant
/// to the operation of shards - such as requests from the gateway to re-connect
/// or invalidate a session - and then pass the events on to the user via an
/// event stream.
///
/// Shards will [go through a queue][`queue`] to initialize new ratelimited
/// sessions with the ratelimit. Refer to Discord's [documentation][docs:shards]
/// on shards to have a better understanding of what they are.
///
/// # Using a shard in multiple tasks
///
/// To use a shard instance in multiple tasks, consider wrapping it in an
/// [`std::sync::Arc`] or [`std::rc::Rc`].
///
/// # Examples
///
/// Create and start a shard and print new and deleted messages:
///
/// ```no_run
/// use futures::stream::StreamExt;
/// use std::env;
/// use twilight_gateway::{EventTypeFlags, Event, Intents, Shard};
///
/// # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Use the value of the "DISCORD_TOKEN" environment variable as the bot's
/// // token. Of course, you may pass this into your program however you want.
/// let token = env::var("DISCORD_TOKEN")?;
/// let event_types = EventTypeFlags::MESSAGE_CREATE | EventTypeFlags::MESSAGE_DELETE;
///
/// let (shard, mut events) = Shard::builder(token, Intents::GUILD_MESSAGES)
///     .event_types(event_types)
///     .build();
///
/// // Start the shard.
/// shard.start().await?;
///
/// // Create a loop of only new message and deleted message events.
///
/// while let Some(event) = events.next().await {
///     match event {
///         Event::MessageCreate(message) => {
///             println!("message received with content: {}", message.content);
///         },
///         Event::MessageDelete(message) => {
///             println!("message with ID {} deleted", message.id);
///         },
///         _ => {},
///     }
/// }
/// # Ok(()) }
/// ```
///
/// [`queue`]: crate::queue
/// [docs:shards]: https://discord.com/developers/docs/topics/gateway#sharding
#[derive(Debug)]
pub struct Shard {
    config: Arc<Config>,
    emitter: Mutex<Option<Emitter>>,
    processor_handle: OnceCell<JoinHandle<()>>,
    session: OnceCell<WatchReceiver<Arc<Session>>>,
}

impl Shard {
    /// Create a new unconfigured shard.
    ///
    /// Use [`start`] to initiate the gateway session.
    ///
    /// # Examples
    ///
    /// Create a new shard and start it, wait a second, and then print its
    /// current connection stage:
    ///
    /// ```no_run
    /// use twilight_gateway::{Intents, Shard};
    /// use std::{env, time::Duration};
    /// use tokio::time as tokio_time;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let token = env::var("DISCORD_TOKEN")?;
    ///
    /// let intents = Intents::GUILD_MESSAGES | Intents::GUILD_MESSAGE_TYPING;
    /// let (shard, _) = Shard::new(token, intents);
    /// shard.start().await?;
    ///
    /// tokio_time::sleep(Duration::from_secs(1)).await;
    ///
    /// let info = shard.info()?;
    /// println!("Shard stage: {}", info.stage());
    /// # Ok(()) }
    /// ```
    ///
    /// [`start`]: Self::start
    pub fn new(token: String, intents: Intents) -> (Self, Events) {
        Self::builder(token, intents).build()
    }

    pub(crate) fn new_with_config(config: Config) -> (Self, Events) {
        let config = Arc::new(config);
        let event_types = config.event_types();

        let (emitter, rx) = Emitter::new(event_types);

        let this = Self {
            config,
            emitter: Mutex::new(Some(emitter)),
            processor_handle: OnceCell::new(),
            session: OnceCell::new(),
        };

        (this, Events::new(event_types, rx))
    }

    /// Create a builder to configure and construct a shard.
    ///
    /// Refer to the builder for more information.
    pub fn builder(token: String, intents: Intents) -> ShardBuilder {
        ShardBuilder::new(token, intents)
    }

    /// Return an immutable reference to the configuration used for this client.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Start the shard, connecting it to the gateway and starting the process
    /// of receiving and processing events.
    ///
    /// The same shard can't be started multiple times. If you stop a shard via
    /// [`shutdown`] or [`shutdown_resumable`] you need to create a new instance
    /// of the shard.
    ///
    /// # Errors
    ///
    /// Returns a [`ShardStartErrorType::AlreadyStarted`] error type if the
    /// shard has already been started.
    ///
    /// Returns a [`ShardStartErrorType::Establishing`] error type if
    /// establishing a connection to the gateway failed.
    ///
    /// Returns a [`ShardStartErrorType::ParsingGatewayUrl`] error type if the
    /// gateway URL couldn't be parsed.
    ///
    /// Returns a [`ShardStartErrorType::RetrievingGatewayUrl`] error type if
    /// the gateway URL couldn't be retrieved from the HTTP API.
    ///
    /// [`shutdown_resumable`]: Self::shutdown_resumable
    /// [`shutdown`]: Self::shutdown
    pub async fn start(&self) -> Result<(), ShardStartError> {
        let url = if let Some(u) = self.config.gateway_url.clone() {
            u.into_string()
        } else {
            // By making an authenticated gateway information retrieval request
            // we're also validating the configured token.
            self.config
                .http_client()
                .gateway()
                .authed()
                .exec()
                .await
                .map_err(|source| ShardStartError {
                    source: Some(Box::new(source)),
                    kind: ShardStartErrorType::RetrievingGatewayUrl,
                })?
                .model()
                .await
                .map_err(|source| ShardStartError {
                    source: Some(Box::new(source)),
                    kind: ShardStartErrorType::RetrievingGatewayUrl,
                })?
                .url
        };

        let emitter = self
            .emitter
            .lock()
            .expect("emitter poisoned")
            .take()
            .ok_or(ShardStartError {
                kind: ShardStartErrorType::AlreadyStarted,
                source: None,
            })?;

        let config = Arc::clone(&self.config);
        let (processor, wrx) =
            ShardProcessor::new(config, url, emitter)
                .await
                .map_err(|source| {
                    let (kind, source) = source.into_parts();

                    let new_kind = match kind {
                        ConnectingErrorType::Establishing => ShardStartErrorType::Establishing,
                        ConnectingErrorType::ParsingUrl { url } => {
                            ShardStartErrorType::ParsingGatewayUrl { url }
                        }
                    };

                    ShardStartError {
                        source,
                        kind: new_kind,
                    }
                })?;

        let handle = tokio::spawn(async {
            processor.run().await;

            tracing::debug!("shard processor future ended");
        });

        // We know that these haven't been set, so we can ignore the result.
        let _res = self.processor_handle.set(handle);
        let _session = self.session.set(wrx);

        Ok(())
    }

    /// Retrieve information about the running of the shard, such as the current
    /// connection stage.
    ///
    /// # Errors
    ///
    /// Returns a [`SessionInactiveError`] if the shard's session is inactive.
    pub fn info(&self) -> Result<Information, SessionInactiveError> {
        let session = self.session()?;

        let (ratelimit_requests, ratelimit_refill) = if let Some(limiter) = session.ratelimit.get()
        {
            (
                limiter.as_ref().map(LeakyBucket::tokens),
                limiter.as_ref().map(LeakyBucket::next_refill),
            )
        } else {
            return Err(SessionInactiveError);
        };

        Ok(Information {
            id: self.config().shard()[0],
            latency: session.heartbeats.latency(),
            ratelimit_refill,
            ratelimit_requests,
            session_id: session.id(),
            seq: session.seq(),
            stage: session.stage(),
        })
    }

    /// Send a command over the gateway.
    ///
    /// # Examples
    ///
    /// Request members whose names start with "tw" in a guild:
    ///
    /// ```no_run
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use std::env;
    /// use twilight_gateway::{shard::Shard, Intents};
    /// use twilight_model::{
    ///     gateway::payload::outgoing::RequestGuildMembers,
    ///     id::Id,
    /// };
    ///
    /// let intents = Intents::GUILD_VOICE_STATES;
    /// let token = env::var("DISCORD_TOKEN")?;
    ///
    /// let (shard, _events) = Shard::new(token, intents);
    /// shard.start().await?;
    ///
    /// // Query members whose names start with "tw" and limit the results to
    /// // 10 members.
    /// let request =
    ///     RequestGuildMembers::builder(Id::new(1))
    ///         .query("tw", Some(10));
    ///
    /// // Send the request over the shard.
    /// shard.command(&request).await?;
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a [`CommandErrorType::Sending`] error type if the message could
    /// not be sent over the websocket. This indicates the shard is currently
    /// restarting.
    ///
    /// Returns a [`CommandErrorType::Serializing`] error type if the provided
    /// value failed to serialize into JSON.
    ///
    /// Returns a [`CommandErrorType::SessionInactive`] error type if the shard
    /// has not been started.
    pub async fn command(&self, value: &impl Command) -> Result<(), CommandError> {
        let json = json::to_vec(value).map_err(|source| CommandError {
            source: Some(Box::new(source)),
            kind: CommandErrorType::Serializing,
        })?;

        self.send(Message::Binary(json))
            .await
            .map_err(CommandError::from_send)
    }

    /// Send a raw websocket message.
    ///
    /// # Examples
    ///
    /// Send a ping message:
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use std::env;
    /// use twilight_gateway::{shard::{raw_message::Message, Shard}, Intents};
    ///
    /// let token = env::var("DISCORD_TOKEN")?;
    /// let (shard, _) = Shard::new(token, Intents::GUILDS);
    /// shard.start().await?;
    ///
    /// shard.send(Message::Ping(Vec::new())).await?;
    /// # Ok(()) }
    /// ```
    ///
    /// Send a normal close (you may prefer to use [`shutdown`]):
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use std::{borrow::Cow, env};
    /// use twilight_gateway::{
    ///     shard::{
    ///         raw_message::{CloseFrame, Message},
    ///         Shard,
    ///     },
    ///     Intents,
    /// };
    ///
    /// let token = env::var("DISCORD_TOKEN")?;
    /// let (shard, _) = Shard::new(token, Intents::GUILDS);
    /// shard.start().await?;
    ///
    /// let close = CloseFrame::from((1000, ""));
    /// let message = Message::Close(Some(close));
    /// shard.send(message).await?;
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a [`SendErrorType::HeartbeaterNotStarted`] error type if the
    /// shard hasn't started the heartbeater.
    ///
    /// Returns a [`SendErrorType::Sending`] error type if there is an issue
    /// with sending via the shard's session. This may occur when the shard is
    /// between sessions.
    ///
    /// Returns a [`SendErrorType::SessionInactive`] error type when the shard
    /// has not been started.
    ///
    /// [`shutdown`]: Self::shutdown
    pub async fn send(&self, message: Message) -> Result<(), SendError> {
        let session = self.session().map_err(|source| SendError {
            source: Some(Box::new(source)),
            kind: SendErrorType::SessionInactive,
        })?;

        // Getting the value of the OnceCell will only return None if it has not been
        // initialized yet, i.e. ratelimiting is enabled and HELLO has not been
        // received yet.
        if let Some(maybe_ratelimiter) = session.ratelimit.get() {
            // The value of the cell has been set, it will be Some if ratelimiting
            // is enabled, else None. We can ignore the second case.
            if let Some(ratelimiter) = maybe_ratelimiter {
                ratelimiter.acquire_one().await;
            }
        } else {
            return Err(SendError {
                kind: SendErrorType::HeartbeaterNotStarted,
                source: None,
            });
        }

        session
            .tx
            .send(message.into_tungstenite())
            .map_err(|source| SendError {
                kind: SendErrorType::Sending,
                source: Some(Box::new(source)),
            })
    }

    /// Shut down the shard.
    ///
    /// The shard will cleanly close the connection by sending a normal close
    /// code, causing Discord to show the bot as being offline. The session will
    /// not be resumable.
    pub fn shutdown(&self) {
        if let Some(processor_handle) = self.processor_handle.get() {
            processor_handle.abort();
        }

        if let Ok(session) = self.session() {
            // Since we're shutting down now, we don't care if it sends or not.
            let _res = session.close(Some(TungsteniteCloseFrame {
                code: CloseCode::Normal,
                reason: "".into(),
            }));
            session.stop_heartbeater();
        }
    }

    /// Shut down the shard in a resumable fashion.
    ///
    /// The shard will cleanly close the connection by sending a restart close
    /// code, causing Discord to keep the bot as showing online. The connection
    /// will be resumable by using the provided session resume information
    /// to [`ClusterBuilder::resume_sessions`].
    ///
    /// [`ClusterBuilder::resume_sessions`]: crate::cluster::ClusterBuilder::resume_sessions
    pub fn shutdown_resumable(&self) -> (u64, Option<ResumeSession>) {
        if let Some(processor_handle) = self.processor_handle.get() {
            processor_handle.abort();
        }

        let shard_id = self.config().shard()[0];

        let session = if let Ok(session) = self.session() {
            session
        } else {
            return (shard_id, None);
        };

        let _res = session.close(Some(TungsteniteCloseFrame {
            code: CloseCode::Restart,
            reason: Cow::from("Closing in a resumable way"),
        }));

        let session_id = session.id();
        let sequence = session.seq.load(Ordering::Relaxed);

        session.stop_heartbeater();

        let data = session_id.map(|id| ResumeSession {
            session_id: id.into_string(),
            sequence,
        });

        (shard_id, data)
    }

    /// Return a handle to the current session.
    ///
    /// # Errors
    ///
    /// Returns a [`SessionInactiveError`] if the shard's session is inactive.
    fn session(&self) -> Result<Arc<Session>, SessionInactiveError> {
        let session = self.session.get().ok_or(SessionInactiveError)?;

        Ok(Arc::clone(&session.borrow()))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CommandError, CommandErrorType, Information, ResumeSession, SendError, SendErrorType,
        SessionInactiveError, Shard, ShardStartError, ShardStartErrorType,
    };
    use static_assertions::{assert_fields, assert_impl_all};
    use std::{error::Error, fmt::Debug};

    assert_impl_all!(CommandErrorType: Debug, Send, Sync);
    assert_impl_all!(CommandError: Error, Send, Sync);
    assert_impl_all!(Information: Clone, Debug, Send, Sync);
    assert_impl_all!(ResumeSession: Clone, Debug, Send, Sync);
    assert_impl_all!(SendErrorType: Debug, Send, Sync);
    assert_impl_all!(SendError: Error, Send, Sync);
    assert_impl_all!(SessionInactiveError: Error, Send, Sync);
    assert_fields!(ShardStartErrorType::ParsingGatewayUrl: url);
    assert_impl_all!(ShardStartErrorType: Debug, Send, Sync);
    assert_impl_all!(ShardStartError: Error, Send, Sync);
    assert_impl_all!(Shard: Debug, Send, Sync);
}
