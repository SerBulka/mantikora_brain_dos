use std::sync::Arc;
use std::sync::Mutex;

use serenity::builder::CreateApplicationCommand;
use serenity::model::prelude::command::CommandOptionType;
use serenity::model::prelude::interaction::application_command::{
    CommandDataOption, CommandDataOptionValue,
};

pub fn run(options: &[CommandDataOption], user_id: Arc<Mutex<u64>>) -> String {
    let option = options
        .get(0)
        .expect("Expected user option")
        .resolved
        .as_ref()
        .expect("Expected user object");

    if let CommandDataOptionValue::User(user, _member) = option {
        let mut data = user_id.lock().unwrap();
        *data = user.id.0;
        format!("Слежу за {}", user.tag())
    } else {
        "Please provide a valid user".to_string()
    }
}

pub fn register(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
    command
        .name("set_target")
        .description("Погавари са мной")
        .create_option(|option| {
            option
                .name("id")
                .description("С кем гаварить?")
                .kind(CommandOptionType::User)
                .required(true)
        })
}
