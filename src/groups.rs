use std::error::Error;

use aws::{extract_saml_accounts, AWSAccountInfo};
use config;
use config::{prompt, Account, Group};
use keycloak::login::get_assertion_response;

use chrono::prelude::*;
use clap::ArgMatches;
use cookie::CookieJar;
use crossterm::style::{style, Color};
use crossterm::Screen;

pub fn command(matches: &ArgMatches) {
    let screen = Screen::default();

    if let Some(_) = matches.subcommand_matches("list") {
        list()
    } else if let Some(matches) = matches.subcommand_matches("delete") {
        let name = matches.value_of("GROUP").unwrap();

        delete(name)
    } else if let Some(matches) = matches.subcommand_matches("add") {
        let cfg = config::load_or_default().unwrap();

        let name = matches.value_of("NAME").unwrap();
        let role = matches.value_of("role").unwrap();
        let append = matches.is_present("append");

        let cfg_username = cfg.username.unwrap();
        let cfg_password = cfg.password.unwrap();
        let username = matches.value_of("username").unwrap_or(&cfg_username);
        let password = matches.value_of("password").unwrap_or(&cfg_password);

        let mfa = matches
            .value_of("mfa")
            .map(|m| m.into())
            .or_else(|| prompt("MFA Token", Some("000000")))
            .expect("No MFA Token provided");

        let session_duration = matches
            .value_of("session_duration")
            .map(|s| s.parse().ok().unwrap());

        let prefix = matches.value_of("prefix");
        let account_names = matches.values_of("accounts");

        if prefix.is_some() && account_names.is_some() {
            println!("Cannot specify both --accounts and --prefix");
            return;
        }

        if prefix.is_none() && account_names.is_none() {
            println!(
                "\nCould not add group {}:\n\n\t{}\n",
                style(name).with(Color::Yellow).into_displayable(&screen),
                style("Must specify either --prefix or --accounts flag")
                    .with(Color::Red)
                    .into_displayable(&screen)
            );
            return;
        }

        let mut accounts: Vec<Account> = vec![];

        print!("Listing allowed roles for your account\t");
        trace!("command.get_assertion_response");

        let mut cookie_jar = CookieJar::new();
        let (_, web_response) = match get_assertion_response(
            &mut cookie_jar,
            &cfg.idp_url,
            username,
            password,
            &mfa,
            true,
        ) {
            Ok(r) => r,
            Err(e) => {
                trace!("command.get_assertion_response.err");
                error!("{:?}", e);
                println!(
                    "{}",
                    style("FAIL").with(Color::Red).into_displayable(&screen)
                );
                println!(
                    "\nCould not add group:\n\n\t{}\n",
                    style(e.description())
                        .with(Color::Red)
                        .into_displayable(&screen)
                );
                return;
            }
        };

        trace!("command.extract_saml_accounts");
        let aws_list = match extract_saml_accounts(&web_response.unwrap()) {
            Ok(l) => l,
            Err(e) => {
                trace!("command.extract_saml_accounts.err");
                error!("{:?}", e);
                println!(
                    "{}",
                    style("FAIL").with(Color::Red).into_displayable(&screen)
                );
                println!(
                    "\nCould not add group:\n\n\t{}\n",
                    style(e.description())
                        .with(Color::Red)
                        .into_displayable(&screen)
                );
                return;
            }
        };

        if let Some(prefix) = prefix {
            accounts = get_acocunts_prefixed_by(&aws_list, prefix, role);
        }
        if let Some(account_names) = account_names {
            accounts =
                get_accounts_by_names(&aws_list, account_names.map(|a| a.into()).collect(), role);
        }

        if accounts.len() == 0 {
            println!(
                "\t{}",
                style("WARNING")
                    .with(Color::Yellow)
                    .into_displayable(&screen)
            );
            println!("\nNo accounts were found with the given parameters. Possible errors:");
            println!("\t- Wrong prefix/accounts used");
            println!("\t- Wrong role used");
        } else {
            println!(
                "\t{}",
                style("SUCCESS")
                    .with(Color::Green)
                    .into_displayable(&screen)
            );
            add(name, session_duration, accounts, append)
        }
    }
}

fn list() {
    let screen = Screen::default();
    let cfg = config::load_or_default().unwrap();

    for (name, group) in &cfg.groups {
        println!(
            "\n{}:",
            style(name).with(Color::Yellow).into_displayable(&screen)
        );

        if let Some(duration) = group.session_duration {
            println!(
                "\t{}: {}",
                "Session Duration",
                style(&format!("{} seconds", duration))
                    .with(Color::Blue)
                    .into_displayable(&screen)
            );
        } else {
            println!(
                "\t{}: {}",
                "Session Duration",
                style("implicit")
                    .with(Color::Blue)
                    .into_displayable(&screen)
            );
        }

        println!("\n\t{}", "Sessions");
        for account in &group.accounts {
            match account.valid_until {
                Some(expiration) => {
                    let now = Local::now();

                    let expiration = expiration.signed_duration_since(now);
                    if expiration.num_minutes() < 0 {
                        println!(
                            "\t{}: {}",
                            &account.name,
                            style("no valid session")
                                .with(Color::Red)
                                .into_displayable(&screen)
                        );
                    } else {
                        println!(
                            "\t{}: {}",
                            &account.name,
                            style(&format!("{} minutes left", expiration.num_minutes()))
                                .with(Color::Green)
                                .into_displayable(&screen)
                        );
                    }
                }
                None => {
                    println!(
                        "\t{}: {}",
                        &account.name,
                        style("no valid session")
                            .with(Color::Red)
                            .into_displayable(&screen)
                    );
                }
            };
        }

        println!("\n\tARNs");
        for account in &group.accounts {
            println!("\t{}: {}", &account.name, account.arn,);
        }
        println!("");
    }
}

fn delete(name: &str) {
    let screen = Screen::default();
    let mut cfg = config::load_or_default().unwrap();

    if !cfg.groups.contains_key(name) {
        println!(
            "\nCould not delete the group {}:\n\n\t{}\n",
            style(name).with(Color::Yellow).into_displayable(&screen),
            style("The specified group does not exist")
                .with(Color::Red)
                .into_displayable(&screen)
        );
        return;
    }
    cfg.groups.remove(name).unwrap();

    cfg.save().unwrap();
    println!(
        "\nSuccessfully deleted group {}.\n",
        style(name).with(Color::Yellow).into_displayable(&screen)
    );
}

fn add(name: &str, session_duration: Option<i64>, accounts: Vec<Account>, append_only: bool) {
    let screen = Screen::default();
    let mut cfg = config::load_or_default().unwrap();

    let mut exists = false;

    if let Some((name, group)) = cfg.groups.iter_mut().find(|&(a, _)| a == name) {
        if append_only {
            println!("Group {} exists, appending new accounts", name);

            let existing_names: Vec<String> = (&group.accounts)
                .into_iter()
                .map(|ref a| a.name.clone())
                .collect();

            group.accounts.extend(
                (&accounts)
                    .into_iter()
                    .filter(|a| !existing_names.contains(&a.name))
                    .map(|a| a.clone())
                    .collect::<Vec<Account>>(),
            );
        } else {
            group.accounts = accounts.clone();
            println!("Group {} exists, replacing accounts", name);
        }
        group.session_duration = session_duration;
        exists = true;
    };

    if !exists {
        println!("Adding group {}", name);

        cfg.groups.insert(
            name.into(),
            Group {
                accounts: accounts,
                session_duration: session_duration,
            },
        );
    }
    println!(
        "\n{}:",
        style(name).with(Color::Yellow).into_displayable(&screen)
    );

    for account in &cfg.groups.get(name).unwrap().accounts {
        println!("\t{}: {}", account.name, account.arn,);
    }

    cfg.save().unwrap();
    println!("\nGroup configuration updated");
}

fn get_acocunts_prefixed_by(
    accounts: &Vec<AWSAccountInfo>,
    prefix: &str,
    role_name: &str,
) -> Vec<Account> {
    accounts
        .into_iter()
        .filter(|a| a.name.starts_with(prefix))
        .filter(|a| a.arn.ends_with(&format!("role/{}", role_name)))
        .map(|a| Account {
            name: a.name.clone(),
            arn: a.arn.clone(),
            valid_until: None,
        })
        .collect()
}

fn get_accounts_by_names(
    accounts: &Vec<AWSAccountInfo>,
    names: Vec<String>,
    role_name: &str,
) -> Vec<Account> {
    accounts
        .into_iter()
        .filter(|a| names.iter().find(|name| *name == &a.name).is_some())
        .filter(|a| a.arn.ends_with(&format!("role/{}", role_name)))
        .map(|a| Account {
            name: a.name.clone(),
            arn: a.arn.clone(),
            valid_until: None,
        })
        .collect()
}
