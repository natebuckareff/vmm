use std::{
    process::{Command, ExitStatus},
    thread,
    time::Duration,
};

use anyhow::{Result, anyhow, bail};
use ipnet::Ipv4Net;

use crate::id::Id;

fn cmd(cmd: &str, args: &[&str]) -> Result<ExitStatus> {
    let ecode = Command::new(cmd).args(args).spawn()?.wait()?;
    Ok(ecode)
}

fn cmd_success(cmd: &str, args: &[&str]) -> Result<ExitStatus> {
    let ecode = Command::new(cmd).args(args).spawn()?.wait()?;
    if !ecode.success() {
        bail!("command failed: {}", cmd)
    }
    Ok(ecode)
}

pub fn get_bridge_name(id: &Id) -> String {
    let id = id.to_string();
    let id = &id[id.len() - 4..];
    format!("vmmbr-{}", id)
}

pub fn get_tap_name(id: &Id) -> String {
    let id = id.to_string();
    let id = &id[id.len() - 4..];
    format!("vmmtap-{}", id)
}

pub fn create_bridge_device(name: &str, ip: Ipv4Net) -> Result<()> {
    dbg!(&name, &ip);
    if cmd("ip", &["link", "show", &name])?.success() {
        println!("IS THIS GETTING CALLED MULTIPLE TIMES???");
        bail!("bridge device already exists: {}", name)
    }
    cmd_success("ip", &["link", "add", &name, "type", "bridge"])?;
    loop {
        let ret = cmd("ip", &["link", "show", &name])?;
        if ret.success() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    cmd_success("ip", &["addr", "add", &ip.to_string(), "dev", &name])?;
    cmd_success("ip", &["link", "set", "up", "dev", &name])?;
    Ok(())
}

pub fn create_tap_device(name: &str, bridge: &str) -> Result<()> {
    dbg!(&name, &bridge);
    cmd_success("ip", &["tuntap", "add", &name, "mode", "tap"])?;
    loop {
        let ret = cmd("ip", &["link", "show", &name])?;
        if ret.success() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    cmd_success("ip", &["link", "set", &name, "up"])?;
    cmd_success("ip", &["link", "set", &name, "master", &bridge])?;
    Ok(())
}

pub fn delete_tap_device(name: &str) -> Result<()> {
    dbg!(&name);
    cmd_success("ip", &["link", "set", &name, "down"])?;
    cmd_success("ip", &["link", "delete", &name])?;
    Ok(())
}

pub fn delete_bridge_device(name: &str) -> Result<()> {
    dbg!(&name);
    cmd_success("ip", &["link", "set", &name, "down"])?;
    cmd_success("ip", &["link", "delete", &name])?;
    Ok(())
}

pub fn get_mac(id: &Id) -> String {
    let id: [u8; 16] = id.into();
    let id = &id[id.len() - 3..];
    format!("52:54:00:{:02x}:{:02x}:{:02x}", id[0], id[1], id[2])
}
