use anyhow::Result;

use crate::cli::Cli;

mod args;
mod cli;
mod ctx;
mod id;
mod image_cache;
mod instance;
mod logger;
mod machine;
mod network;
mod progress_router;
mod server;
mod share_dir;
mod task_actor;
mod task_group;
mod text_table;
mod vmm_dirs;

fn main() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let cli = Cli::new();
        if let Err(e) = cli.run().await {
            eprintln!("{}", e);
        }
    });
    Ok(())

    /*
    let ctx = Ctx::new();
    let mut server = Server::new();
    let mut task_group = TaskGroup::new(ctx.cancel_token().clone());

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let ctx2 = ctx.clone();
        ctrlc::set_handler(move || {
            println!("CTRL-C");
            ctx2.cancel_token().cancel();
        })
        .unwrap();

        let ctx = ctx.with_progress_router(create_progress_router(&mut task_group));

        let image_cache = create_image_cache(ctx.clone(), &mut task_group);
        let ctx = ctx.with_image_manager(image_cache);

        let network = Network::read(&ctx, Id::from_str("ACKonMM9amvHoRz5f8ucc8").unwrap())
            .await
            .unwrap();

        let machine = Machine::read(&ctx, Id::from_str("kqko0nWNyvXNYG7DrIVhV9").unwrap())
            .await
            .unwrap();

        let instance = Instance::read(&ctx, Id::from_str("Cu4HKxEJouYimccDrcAJE8").unwrap())
            .await
            .unwrap();

        println!("network: {:?}", network);
        println!("machine: {:?}", machine);
        println!("instance: {:?}", instance.id());

        let ctx2 = ctx.clone();
        tokio::spawn(async move {
            let mut progress_bars = HashMap::new();
            let mut progress_receiver = ctx2.progress_router().subscribe();

            while let Ok(progress) = progress_receiver.recv().await {
                match progress {
                    ProgressMessage::Start(id, Some(total)) => {
                        let pb = ProgressBar::new(total);

                        pb.set_style(
                            ProgressStyle::with_template(
                                "[{elapsed_precise}] [{wide_bar}] {bytes}/{total_bytes} ({eta})",
                            )
                            .unwrap()
                            .with_key("eta", |state: &ProgressState, w: &mut dyn Write| {
                                write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap()
                            })
                            .progress_chars("#>-"),
                        );

                        pb.set_position(0);

                        progress_bars.insert(id, (pb, 0u64));
                    }
                    ProgressMessage::Start(id, None) => {
                        todo!();
                    }
                    ProgressMessage::Update(id, count) => {
                        if let Some((pb, sofar)) = progress_bars.get_mut(&id) {
                            *sofar += count;
                            pb.set_position(*sofar);
                        }
                    }
                    ProgressMessage::Finish(id) => {
                        if let Some((pb, _)) = progress_bars.remove(&id) {
                            pb.finish_with_message("downloaded");
                        }
                    }
                }
            }
        });

        server.read_all(&ctx).await.unwrap();
        server.start_instance(&ctx, instance.id()).await.unwrap();

        println!("SHOULDNT THIS BE WAITING????");

        task_group.wait().await;
    });
    */
}
