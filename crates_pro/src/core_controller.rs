//! the core controller module
//! It receives messages from Kafka MQ one by one,
//! parse them, and store it into tugraph, and notify
//! other processes.

use analysis::analyse_once;
#[allow(unused_imports)]
use data_transporter::{run_api_server, Transporter};
use repo_import::ImportDriver;

use crate::cli::CratesProCli;
#[allow(unused_imports)]
use std::{env, fs, sync::Arc, time::Duration};
use tokio::sync::Mutex;

pub struct CoreController {
    pub cli: CratesProCli,

    pub import: bool,
    pub analysis: bool,
    pub package: bool,
}
struct SharedState {
    is_packaging: bool,
}

impl CoreController {
    pub async fn new(cli: CratesProCli) -> Self {
        let import = env::var("CRATES_PRO_IMPORT").unwrap().eq("1");
        let analysis = env::var("CRATES_PRO_ANALYSIS").unwrap().eq("1");
        let package = env::var("CRATES_PRO_PACKAGE").unwrap().eq("1");
        Self {
            cli,
            import,
            analysis,
            package,
        }
    }

    pub async fn run(&self) {
        let import = self.import;
        let analysis = self.analysis;
        let package = self.package;

        let shared_state: Arc<tokio::sync::Mutex<SharedState>> =
            Arc::new(Mutex::new(SharedState {
                is_packaging: false,
            }));

        let dont_clone = self.cli.dont_clone;

        let state_clone1: Arc<tokio::sync::Mutex<SharedState>> = Arc::clone(&shared_state);
        let import_task = tokio::spawn(async move {
            if import {
                let should_reset_kafka_offset =
                    env::var("SHOULD_RESET_KAFKA_OFFSET").unwrap().eq("1");
                if should_reset_kafka_offset {
                    repo_import::reset_kafka_offset()
                        .await
                        .unwrap_or_else(|x| panic!("{}", x));
                }

                // conduct repo parsing and importing
                let mut import_driver = ImportDriver::new(dont_clone).await;
                let mut count = 0;
                loop {
                    let mut state = state_clone1.lock().await;
                    while state.is_packaging {
                        drop(state); // 释放锁以便等待
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        state = state_clone1.lock().await; // 重新获取锁
                    }

                    let _ = import_driver.import_from_mq_for_a_message().await;
                    count += 1;
                    //let _ = import_driver.context.write_tugraph_import_files();
                    if count == 1000 {
                        import_driver.context.depends_on.clone_from(
                            &(import_driver
                                .context
                                .version_updater
                                .to_depends_on_edges()
                                .await),
                        );
                        //let _ = import_driver.context.update_max_version().await.unwrap();
                        import_driver.context.write_tugraph_import_files();
                        count = 0;
                    }
                    drop(state);

                    tokio::time::sleep(Duration::from_secs(0)).await;
                }
            }
        });

        let state_clone2: Arc<tokio::sync::Mutex<SharedState>> = Arc::clone(&shared_state);
        let analyze_task = tokio::spawn(async move {
            if analysis {
                loop {
                    let mut state = state_clone2.lock().await;
                    while state.is_packaging {
                        drop(state);
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        state = state_clone2.lock().await;
                    }
                    drop(state);
                    //println!("Analyzing crate...");

                    let output_dir_path = "/home/rust/output/analysis";

                    /*match fs::create_dir(output_dir_path) {
                        Ok(_) => {}
                        Err(_) => {}
                    }*/

                    let _ = analyse_once(output_dir_path).await;

                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        });
        #[allow(unused_variables)]
        let state_clone3: Arc<tokio::sync::Mutex<SharedState>> = Arc::clone(&shared_state);
        let package_task = tokio::spawn(async move {
            if package {
                /*loop {
                    {
                        let mut state = state_clone3.lock().await;
                        state.is_packaging = true;
                    }

                    // process here

                    {
                        let mut transporter = Transporter::new(
                            &tugraph_bolt_url,
                            &tugraph_user_name,
                            &tugraph_user_password,
                            &tugraph_cratespro_db,
                        )
                        .await;

                        transporter.transport_data().await.unwrap();
                    }

                    {
                        let mut state = state_clone3.lock().await;
                        state.is_packaging = false;
                    }

                    // after one hour
                    tokio::time::sleep(Duration::from_secs(72000)).await;
                }*/
            }
        });

        if package {
            let tugraph_bolt_url = env::var("TUGRAPH_BOLT_URL").unwrap();
            let tugraph_user_name = env::var("TUGRAPH_USER_NAME").unwrap();
            let tugraph_user_password = env::var("TUGRAPH_USER_PASSWORD").unwrap();
            let tugraph_cratespro_db = env::var("TUGRAPH_CRATESPRO_DB").unwrap();
            run_api_server(
                &tugraph_bolt_url,
                &tugraph_user_name,
                &tugraph_user_password,
                &tugraph_cratespro_db,
            )
            .await
            .unwrap();
        }

        import_task.await.unwrap();
        analyze_task.await.unwrap();
        package_task.await.unwrap();
    }
}
