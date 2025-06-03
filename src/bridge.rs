use std::thread;

use anyhow::{Context, Result};
use futures::{TryStreamExt, executor::block_on};
use rtnetlink::new_connection;

fn ensure_bridge(name: &str) -> Result<u32> {
    block_on(async {
        let (conn, handle, _) = new_connection().context("open netlink connection")?;

        thread::spawn(move || {
            let _ = block_on(conn);
        });

        if let Some(link) = handle
            .link()
            .get()
            .match_name(name.to_string())
            .execute()
            .try_next()
            .await?
        {
            handle.link().set(link.header.index).up().execute().await?;
        }

        // Otherwise create it
        handle
            .link()
            .add()
            .bridge(name.to_string())
            .execute()
            .await?;

        // look it up once more to get its index
        let idx = handle
            .link()
            .get()
            .match_name(name.to_string())
            .execute()
            .try_next()
            .await?
            .ok_or_else(|| anyhow::anyhow!("bridge vanished"))?
            .header
            .index;

        handle.link().set(idx).up().execute().await?;
        Ok(idx)
    })
}
