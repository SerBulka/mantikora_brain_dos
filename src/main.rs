mod commands;

use dotenv::dotenv;

use std::sync::Arc;
use std::sync::Mutex;

use serenity::async_trait;
use serenity::model::application::interaction::{Interaction, InteractionResponseType};
use serenity::model::gateway::Ready;
use serenity::model::id::GuildId;
use serenity::prelude::*;
use songbird::{
    driver::DecodeMode,
    model::payload::{ClientDisconnect, Speaking},
    Config,
    CoreEvent,
    Event,
    EventContext,
    EventHandler as VoiceEventHandler,
    SerenityInit,
};
use serenity::client::Context;

struct Handler {
    user_id: Arc<Mutex<u64>>
}


#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            println!("Received command interaction: {:#?}", command);

            let content = match command.data.name.as_str() {
                "id" => commands::id::run(&command.data.options),
                "set_target" => commands::set_target::run(&command.data.options, Arc::clone(&self.user_id)),
                "stop" => play(&ctx).await,
                "start" => join(&ctx).await,
                _ => "not implemented :(".to_string(),
            };

            if let Err(why) = command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| message.content(content))
                })
                .await
            {
                println!("Cannot respond to slash command: {}", why);
            }
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
        let guild_id: u64 = std::env::var("GUILD_ID").unwrap().parse().unwrap();
        let guild_id = GuildId(guild_id);

        let commands = GuildId::set_application_commands(&guild_id, &ctx.http, |commands| {
            commands
                .create_application_command(|command| commands::id::register(command))
                .create_application_command(|command| commands::start::register(command))
                .create_application_command(|command| commands::stop::register(command))
                .create_application_command(|command| commands::set_target::register(command))
        })
        .await;

        println!("I now have the following guild slash commands: {:#?}", commands);
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    
    dotenv().ok();
    // Configure the client with your Discord bot token in the environment.
    let token: String = std::env::var("DISCORD_TOKEN").unwrap();
    let guild_id: u64 = std::env::var("GUILD_ID").unwrap().parse().unwrap();

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::non_privileged();

    let handler = Handler{ user_id: Arc::new(Mutex::new(0))};
    // Build our client.
    let mut client = Client::builder(token, intents)
        .event_handler(handler)
        .register_songbird()
        .await
        .expect("Error creating client");

    // Finally, start a single shard, and start listening to events.
    //
    // Shards will automatically attempt to reconnect, and will perform
    // exponential backoff until it reconnects.
    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}

struct Receiver;

impl Receiver {
    pub fn new() -> Self {
        // You can manage state here, such as a buffer of audio packet bytes so
        // you can later store them in intervals.
        Self { }
    }
}

#[async_trait]
impl VoiceEventHandler for Receiver {
    #[allow(unused_variables)]
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        use EventContext as Ctx;
        match ctx {
            Ctx::SpeakingStateUpdate(
                Speaking {speaking, ssrc, user_id, ..}
            ) => {
                println!(
                    "Speaking state update: user {:?} has SSRC {:?}, using {:?}",
                    user_id,
                    ssrc,
                    speaking,
                );
            },
            Ctx::SpeakingUpdate(data) => {
                // You can implement logic here which reacts to a user starting
                // or stopping speaking, and to map their SSRC to User ID.
                println!(
                    "Source {} has {} speaking.",
                    data.ssrc,
                    if data.speaking {"started"} else {"stopped"},
                );
            },
            Ctx::VoicePacket(data) => {
                // An event which fires for every received audio packet,
                // containing the decoded data.
                if let Some(audio) = data.audio {
                    println!("Audio packet's first 5 samples: {:?}", audio.get(..5.min(audio.len())));
                    println!(
                        "Audio packet sequence {:05} has {:04} bytes (decompressed from {}), SSRC {}",
                        data.packet.sequence.0,
                        audio.len() * std::mem::size_of::<i16>(),
                        data.packet.payload.len(),
                        data.packet.ssrc,
                    );
                } else {
                    println!("RTP packet, but no audio. Driver may not be configured to decode.");
                }
            },
            Ctx::RtcpPacket(data) => {
                // An event which fires for every received rtcp packet,
                // containing the call statistics and reporting information.
                println!("RTCP packet received: {:?}", data.packet);
            },
            Ctx::ClientDisconnect(
                ClientDisconnect {user_id, ..}
            ) => {
                // You can implement your own logic here to handle a user who has left the
                // voice channel e.g., finalise processing of statistics etc.
                // You will typically need to map the User ID to their SSRC; observed when
                // first speaking.

                println!("Client disconnected: user {:?}", user_id);
            },
            _ => {
                // We won't be registering this struct for any more event classes.
                unimplemented!()
            }
        }

        None
    }
}


async fn join(ctx: &Context) -> String {
    let connect_to: u64 = std::env::var("CHANNEL_ID").unwrap().parse().unwrap();
    let guild_id: u64 = std::env::var("GUILD_ID").unwrap().parse().unwrap();

    let manager = songbird::get(ctx).await
    .expect("Songbird Voice client placed in at initialisation.").clone();

    let (handler_lock, conn_result) = manager.join(guild_id, connect_to).await;

    if let Ok(_) = conn_result {
        // NOTE: this skips listening for the actual connection result.
        let mut handler = handler_lock.lock().await;

        handler.add_global_event(
            CoreEvent::SpeakingStateUpdate.into(),
            Receiver::new(),
        );

        handler.add_global_event(
            CoreEvent::SpeakingUpdate.into(),
            Receiver::new(),
        );

        handler.add_global_event(
            CoreEvent::VoicePacket.into(),
            Receiver::new(),
        );

        handler.add_global_event(
            CoreEvent::RtcpPacket.into(),
            Receiver::new(),
        );

        handler.add_global_event(
            CoreEvent::ClientDisconnect.into(),
            Receiver::new(),
        );
        println!("тама");
        "Присоединился к каналу".to_string()
    } else {
        println!("тута");
        "Не присоединился к каналу".to_string()
    }
}

async fn play(ctx: &Context) -> String {
    let url = "D\\mantikora_brain_dos\\mp3\\stream.mp3".to_string();
    // let url = "https://www.youtube.com/watch?v=VXDTjM67d30&pp=ygUP0L7RgiDQstC40L3RgtCw".to_string();

   
    let guild_id: u64 = std::env::var("GUILD_ID").unwrap().parse().unwrap();

    let manager = songbird::get(ctx).await
        .expect("Songbird Voice client placed in at initialisation.").clone();

        let handler_lock = manager.join(guild_id, 701137077168898170).await;
        let mut handler = handler_lock.0.lock().await;

        let source = match songbird::ffmpeg(&url).await {
            Ok(source) => source,
            Err(why) => {
                println!("Err starting source: {:?}", why);

                return "Я русский ZZZ".to_string()
            },
        };

        handler.play_source(source);

        return "Я".to_string()

}
