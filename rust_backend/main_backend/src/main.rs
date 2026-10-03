#![feature(thread_id_value)]
#![allow(clippy::type_complexity)]

use dlopen2::wrapper::WrapperApi;
use std::io::Read;

static MAIN_TOKIO_RUNTIME: once_cell::sync::Lazy<tokio::runtime::Runtime> = once_cell::sync::Lazy::new(
    || { tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap() }
);

static MAIN_BACKEND_PANIC_FLAG: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static MAIN_BACKEND_PANIC_NOTIFY: std::sync::LazyLock<tokio::sync::Notify> =
    std::sync::LazyLock::new(tokio::sync::Notify::new);

// TODO: Use Rc for better performance
#[derive(serde::Deserialize, serde::Serialize)]
struct SingleModuleConfigJson {
    name: String,
    id: String,
    protocol: String,
    enabled: bool,
    dependencies: Vec<String>,
    unload_timeout: usize,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ModuleConfigJson {
    working_load: std::collections::HashMap<String, SingleModuleConfigJson>,
    restricted_mode: bool,
}

type AsyncModifiable<T> = std::sync::Arc<tokio::sync::Mutex<T>>;

fn new_async_modifiable<T>(x: T) -> AsyncModifiable<T> {
    std::sync::Arc::new(tokio::sync::Mutex::new(x))
}

#[derive(dlopen2::wrapper::WrapperApi)]
struct ModuleInstance {
    on_init: extern "Rust" fn(
        global_module_statuses_by_protocol: AsyncModifiable<
            std::collections::HashMap<String, AsyncModifiable<ModuleStatus>>
        >
    ) -> (tokio::runtime::Runtime, AsyncModifiable<ModuleStatus>),
    on_unload: extern "Rust" fn(unload_timeout_ms: usize),
}

#[derive(Debug)]
pub struct ModuleStatus {
    initialized: bool,
    panicked: bool,
    socket_port: AsyncModifiable<u16>,
    // Lets waiters for `initialized` block on a notification instead of polling on a timer.
    // notify_one()'s buffered-permit semantics make this race-free regardless of whether the
    // notify happens before or after a waiter starts waiting.
    init_notify: std::sync::Arc<tokio::sync::Notify>,
}

struct ModuleCombination {
    name: String,
    instance: dlopen2::wrapper::Container<ModuleInstance>,
    tokio_runtime: tokio::runtime::Runtime,
}

fn main() {
    // Modules are loaded from separately-compiled dylibs, each statically linking their own
    // copy of tokio. Relying on `tokio::signal::ctrl_c()` here was empirically unreliable once
    // those modules' own runtimes were doing real socket I/O alongside ours — the signal would
    // sometimes never resolve the future at all. A raw OS-level flag set directly by `signal-hook`
    // (no async runtime involved in *noticing* the signal, only in polling the flag) is robust to
    // however many separate tokio copies exist in the process.
    let sigint_received: std::sync::Arc<std::sync::atomic::AtomicBool> = std::sync::Arc::new(
        std::sync::atomic::AtomicBool::new(false)
    );
    signal_hook::flag::register(signal_hook::consts::SIGINT, sigint_received.clone()).unwrap();

    let module_combinations: AsyncModifiable<Vec<ModuleCombination>> = new_async_modifiable(vec![]);
    let module_statuses_by_protocol: AsyncModifiable<
        std::collections::HashMap<String, AsyncModifiable<ModuleStatus>>
    > = new_async_modifiable(std::collections::HashMap::new());
    let module_config_json: AsyncModifiable<ModuleConfigJson> = MAIN_TOKIO_RUNTIME.block_on(async {
        new_async_modifiable(parse_module_config_json().await) // Parse from module config json string.
    });
    {
        let module_combinations: AsyncModifiable<Vec<ModuleCombination>> =
            module_combinations.clone();
        let module_statuses_by_protocol: AsyncModifiable<
            std::collections::HashMap<String, AsyncModifiable<ModuleStatus>>
        > = module_statuses_by_protocol.clone();
        let module_config_json: AsyncModifiable<ModuleConfigJson> = module_config_json.clone();
        MAIN_TOKIO_RUNTIME.block_on(async move {
            let guard_module_config_json: tokio::sync::MutexGuard<
                '_,
                ModuleConfigJson
            > = module_config_json.lock().await;
            load_modules(
                module_combinations,
                module_statuses_by_protocol,
                &guard_module_config_json
            ).await; // Load modules.
            drop(guard_module_config_json);
        });
    }

    {
        let module_combinations: AsyncModifiable<Vec<ModuleCombination>> =
            module_combinations.clone();
        let module_config_json: AsyncModifiable<ModuleConfigJson> = module_config_json.clone();
        MAIN_TOKIO_RUNTIME.spawn(async move {
            loop {
                let guard_module_combinations: tokio::sync::MutexGuard<
                    '_,
                    Vec<ModuleCombination>
                > = module_combinations.lock().await;
                let guard_module_statuses: tokio::sync::MutexGuard<
                    '_,
                    std::collections::HashMap<String, AsyncModifiable<ModuleStatus>>
                > = module_statuses_by_protocol.lock().await;
                let guard_module_config_json: tokio::sync::MutexGuard<
                    '_,
                    ModuleConfigJson
                > = module_config_json.lock().await;
                for (index, status) in guard_module_statuses.iter().enumerate() {
                    let status: &AsyncModifiable<ModuleStatus> = status.1;
                    let guard_status: tokio::sync::MutexGuard<
                        '_,
                        ModuleStatus
                    > = status.lock().await;
                    if guard_status.panicked {
                        println!(
                            "{}",
                            ansi_term::Color::Yellow.paint(
                                format!(
                                    "[MAIN_BACKEND] [WARNING] [THREAD {}] [FILE `{}` LINE {}] Module {} has panicked.",
                                    std::thread::current().id().as_u64(),
                                    file!(),
                                    line!(),
                                    guard_module_combinations[index].name
                                )
                            )
                        );
                        drop(guard_status);
                        if guard_module_config_json.restricted_mode {
                            eprintln!(
                                "{}",
                                ansi_term::Color::Red.paint(
                                    format!(
                                        "[MAIN_BACKEND] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Due to the restricted mode, the server backend now is shutting down.",
                                        std::thread::current().id().as_u64(),
                                        file!(),
                                        line!()
                                    )
                                )
                            );
                            drop(guard_module_config_json);
                            println!(
                                "{}",
                                ansi_term::Color::Blue.paint(
                                    format!(
                                        "[MAIN_BACKEND] [INFO] [THREAD {}] [FILE `{}` LINE {}] Now quitting...",
                                        std::thread::current().id().as_u64(),
                                        file!(),
                                        line!()
                                    )
                                )
                            );
                            MAIN_BACKEND_PANIC_FLAG.store(
                                true,
                                std::sync::atomic::Ordering::Release,
                            );
                            MAIN_BACKEND_PANIC_NOTIFY.notify_waiters();
                            panic!();
                        }
                    } else {
                        drop(guard_status);
                    }
                }
                drop(guard_module_combinations);
                drop(guard_module_config_json);
                drop(guard_module_statuses);
                tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            }
        });
    }

    {
        let module_combinations: AsyncModifiable<Vec<ModuleCombination>> =
            module_combinations.clone();
        let module_config_json: AsyncModifiable<ModuleConfigJson> = module_config_json.clone();
        MAIN_TOKIO_RUNTIME.block_on(async move {
            // Wait for Ctrl+C (SIGINT), polling the raw OS-level flag rather than
            // tokio::signal::ctrl_c() — see the comment where `sigint_received` is registered.
            while !sigint_received.load(std::sync::atomic::Ordering::Relaxed) {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            let mut guard_module_combinations: tokio::sync::MutexGuard<
                '_,
                Vec<ModuleCombination>
            > = module_combinations.lock().await;
            let guard_module_config_json: tokio::sync::MutexGuard<
                '_,
                ModuleConfigJson
            > = module_config_json.lock().await;
            tokio::task::block_in_place(|| {
                unload_module_combinations(
                    &mut guard_module_combinations,
                    &guard_module_config_json,
                );
            });
            drop(guard_module_config_json);
            drop(guard_module_combinations);
            println!(
                "{}",
                ansi_term::Color::Purple.paint(
                    format!(
                        "[MAIN_BACKEND] [DOWN] [THREAD {}] [FILE `{}` LINE {}] Successfully unloaded all the modules.",
                        std::thread::current().id().as_u64(),
                        file!(),
                        line!()
                    )
                )
            );
            println!(
                "{}",
                ansi_term::Color::Purple.paint(
                    format!(
                        "[MAIN_BACKEND] [DOWN] [THREAD {}] [FILE `{}` LINE {}] Now quitting...",
                        std::thread::current().id().as_u64(),
                        file!(),
                        line!()
                    )
                )
            );
            std::process::exit(0);
        });
    }

    {
        let module_combinations: AsyncModifiable<Vec<ModuleCombination>> =
            module_combinations.clone();
        let module_config_json: AsyncModifiable<ModuleConfigJson> = module_config_json.clone();
        MAIN_TOKIO_RUNTIME.block_on(async move {
            loop {
                let notified = MAIN_BACKEND_PANIC_NOTIFY.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                let panic_detected = MAIN_BACKEND_PANIC_FLAG.load(
                    std::sync::atomic::Ordering::Acquire,
                );
                if panic_detected {
                    break;
                }
                notified.await;
            }
            let mut guard_module_combinations: tokio::sync::MutexGuard<
                '_,
                Vec<ModuleCombination>
            > = module_combinations.lock().await;
            let guard_module_config_json: tokio::sync::MutexGuard<
                '_,
                ModuleConfigJson
            > = module_config_json.lock().await;
            tokio::task::block_in_place(|| {
                unload_module_combinations(
                    &mut guard_module_combinations,
                    &guard_module_config_json,
                );
            });
            drop(guard_module_config_json);
            drop(guard_module_combinations);
            println!(
                "{}",
                ansi_term::Color::Blue.paint(
                    format!(
                        "[MAIN_BACKEND] [INFO] [THREAD {}] [FILE `{}` LINE {}] Successfully unloaded all the modules.",
                        std::thread::current().id().as_u64(),
                        file!(),
                        line!()
                    )
                )
            );
            println!(
                "{}",
                ansi_term::Color::Blue.paint(
                    format!(
                        "[MAIN_BACKEND] [INFO] [THREAD {}] [FILE `{}` LINE {}] Now quitting...",
                        std::thread::current().id().as_u64(),
                        file!(),
                        line!()
                    )
                )
            );
            std::process::exit(0);
        });
    }
}

fn get_parent_path() -> String {
    // Get the parent path
    let mut pwd: std::path::PathBuf = std::env::current_dir().unwrap();
    pwd.pop();
    String::from(pwd.to_str().unwrap())
}

async fn parse_module_config_json() -> ModuleConfigJson {
    let mut module_config_json_string: String = String::new();
    let module_config_json_file_path: String = get_parent_path() + "/module_config_rs.json";
    let mut module_config_json_file: std::fs::File = match
        std::fs::File::open(&module_config_json_file_path)
    {
        Ok(file) => file,
        Err(e) => {
            eprintln!(
                "{}",
                ansi_term::Color::Red.paint(
                    format!(
                        "[MAIN_BACKEND] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Failed to open module_config_rs.json from `{}`. Maybe the file doesn't exist?",
                        std::thread::current().id().as_u64(),
                        file!(),
                        line!(),
                        module_config_json_file_path
                    )
                )
            );
            eprintln!(
                "{}",
                ansi_term::Color::Red.paint(
                    format!(
                        "[MAIN_BACKEND] [ERROR] [THREAD {}] [FILE `{}` LINE {}] {}",
                        std::thread::current().id().as_u64(),
                        file!(),
                        line!(),
                        e
                    )
                )
            );
            MAIN_BACKEND_PANIC_FLAG.store(
                true,
                std::sync::atomic::Ordering::Release,
            );
            panic!();
        }
    };
    module_config_json_file.read_to_string(&mut module_config_json_string).unwrap(); // Read module config json file to string.

    serde_json::from_str(module_config_json_string.as_str()).unwrap()
}

struct ForwardStarRepresentation {
    edge_cnt: usize,
    edges: Vec<ForwardStarRepresentationEdge>,
    head: Vec<Option<usize>>,
}

struct ForwardStarRepresentationEdge {
    nxt: usize,
    to: usize,
}

impl ForwardStarRepresentation {
    fn new(node_cnt: usize) -> ForwardStarRepresentation {
        let mut head: Vec<Option<usize>> = Vec::with_capacity(node_cnt + 1);
        for _ in 0..=node_cnt {
            head.push(None);
        }
        let mut edges: Vec<ForwardStarRepresentationEdge> = Vec::with_capacity(node_cnt);
        edges.push(ForwardStarRepresentationEdge { nxt: 0, to: 0 });
        ForwardStarRepresentation {
            edge_cnt: 0,
            edges,
            head,
        }
    }
    fn add_edge(&mut self, from: usize, to: usize) {
        self.edge_cnt += 1;
        self.edges.push(ForwardStarRepresentationEdge {
            nxt: self.head[from].unwrap_or(0),
            to,
        });
        self.head[from] = Some(self.edge_cnt);
    }
}

fn check_protocol_version_requirements_satisfied(
    required_version: &String,
    given_version: &String
) -> bool {
    required_version == given_version
}

async fn load_modules(
    module_combinations: AsyncModifiable<Vec<ModuleCombination>>,
    module_statuses_by_protocol: AsyncModifiable<
        std::collections::HashMap<String, AsyncModifiable<ModuleStatus>>
    >,
    module_config_json: &ModuleConfigJson
) {
    // Giving ID to each module.
    let mut ids_by_module_protocol: std::collections::HashMap<
        String,
        usize
    > = std::collections::HashMap::new();
    let mut modules_protocol_version: std::collections::HashMap<
        String,
        String
    > = std::collections::HashMap::new();
    let mut modules_name_by_id: std::collections::HashMap<
        usize,
        String
    > = std::collections::HashMap::new();
    let mut modules_protocol_without_version_by_id: std::collections::HashMap<
        usize,
        String
    > = std::collections::HashMap::new();
    let mut id_cnt: usize = 0;
    for config in module_config_json.working_load.values() {
        if config.enabled {
            id_cnt += 1;
            let pos: usize = config.protocol.find('@').unwrap();
            let module_protocol_without_version: String = String::from(&config.protocol[..pos]); // TODO: Use Rc for better performance
            if
                ids_by_module_protocol
                    .insert(module_protocol_without_version.clone(), id_cnt)
                    .is_some()
            {
                eprintln!(
                    "{}",
                    ansi_term::Color::Red.paint(
                        format!(
                            "[MAIN_BACKEND] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Encountered different modules implemented the same protocol {}.",
                            std::thread::current().id().as_u64(),
                            file!(),
                            line!(),
                            module_protocol_without_version
                        )
                    )
                );
                panic!();
            }

            let module_protocol_with_only_version: String = String::from(
                &config.protocol[pos + 1..]
            ); // TODO: Use Rc for better performance
            if
                modules_protocol_version
                    .insert(
                        module_protocol_without_version.clone(),
                        module_protocol_with_only_version
                    )
                    .is_some()
            {
                eprintln!(
                    "{}",
                    ansi_term::Color::Red.paint(
                        format!(
                            "[MAIN_BACKEND] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Encountered different modules implemented the same protocol {}.",
                            std::thread::current().id().as_u64(),
                            file!(),
                            line!(),
                            module_protocol_without_version
                        )
                    )
                );
                panic!();
            }

            if
                modules_name_by_id
                    .insert(id_cnt, config.id.clone()) // TODO: Use Rc for better performance
                    .is_some()
            {
                eprintln!(
                    "{}",
                    ansi_term::Color::Red.paint(
                        format!(
                            "[MAIN_BACKEND] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Encountered different modules implemented the same protocol {}.",
                            std::thread::current().id().as_u64(),
                            file!(),
                            line!(),
                            module_protocol_without_version
                        )
                    )
                );
                panic!();
            }

            if
                modules_protocol_without_version_by_id
                    .insert(id_cnt, module_protocol_without_version.clone()) // TODO: Use Rc for better performance
                    .is_some()
            {
                eprintln!(
                    "{}",
                    ansi_term::Color::Red.paint(
                        format!(
                            "[MAIN_BACKEND] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Encountered different modules implemented the same protocol {}.",
                            std::thread::current().id().as_u64(),
                            file!(),
                            line!(),
                            module_protocol_without_version
                        )
                    )
                );
                panic!();
            }
        }
    }
    // Build dependencies DAG.
    let mut forward_star_representation: ForwardStarRepresentation =
        ForwardStarRepresentation::new(id_cnt);
    let mut indegs: Vec<usize> = vec![0; id_cnt + 1];
    for config in module_config_json.working_load.values() {
        if config.enabled {
            for dependencies_protocol in &config.dependencies {
                let pos: usize = dependencies_protocol.find('@').unwrap();
                let dependencies_protocol_without_version: String = String::from(
                    &dependencies_protocol[..pos]
                );
                if
                    check_protocol_version_requirements_satisfied(
                        &String::from(&dependencies_protocol[pos + 1..]),
                        modules_protocol_version
                            .get(&dependencies_protocol_without_version)
                            .unwrap_or_else(|| {
                                eprintln!(
                                    "{}",
                                    ansi_term::Color::Red.paint(
                                        format!(
                                            "[MAIN_BACKEND] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Protocol requirements are not satisfied.",
                                            std::thread::current().id().as_u64(),
                                            file!(),
                                            line!()
                                        )
                                    )
                                );
                                eprintln!(
                                    "{}",
                                    ansi_term::Color::Red.paint(
                                        format!(
                                            "[MAIN_BACKEND] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Requires protocol `{}`.",
                                            std::thread::current().id().as_u64(),
                                            file!(),
                                            line!(),
                                            dependencies_protocol_without_version
                                        )
                                    )
                                );
                                panic!();
                            })
                    )
                {
                    let result: Option<&usize> = ids_by_module_protocol.get(
                        &dependencies_protocol_without_version
                    );
                    if let Some(id_by_module_protocol) = result {
                        let pos: usize = config.protocol.find('@').unwrap();
                        let protocol_without_version: String = String::from(
                            &config.protocol[..pos]
                        );
                        let to: usize = *ids_by_module_protocol
                            .get(&protocol_without_version)
                            .unwrap();
                        forward_star_representation.add_edge(*id_by_module_protocol, to);
                        indegs[to] += 1;
                    } else {
                        panic!();
                    }
                }
            }
        }
    }
    // Load modules.
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for (index, value) in indegs
        .iter()
        .enumerate()
        .take(id_cnt + 1)
        .skip(1) {
        if *value == 0 {
            queue.push_back(index);
        }
    }
    while !queue.is_empty() {
        let head_ele: usize = queue.pop_front().unwrap();
        let module_name: &String = modules_name_by_id.get(&head_ele).unwrap();
        println!(
            "{}",
            ansi_term::Color::Blue.paint(
                format!(
                    "[MAIN_BACKEND] [INFO] [THREAD {}] [FILE `{}` LINE {}] Loading module {}...",
                    std::thread::current().id().as_u64(),
                    file!(),
                    line!(),
                    module_name
                )
            )
        );

        let module_config = module_config_json.working_load.get(module_name).unwrap();
        let module_library_file_path: String =
            get_parent_path() +
            (if cfg!(target_os = "windows") {
                "\\rust_backend\\modules\\"
            } else {
                "/rust_backend/modules/"
            }) +
            &module_config.id +
            (if cfg!(target_os = "windows") { "\\" } else { "/lib" }) +
            &module_config.id +
            (if cfg!(target_os = "windows") { ".dll" } else { ".so" }); // Get the path of the module.

        let module: Result<dlopen2::wrapper::Container<ModuleInstance>, dlopen2::Error> = unsafe {
            dlopen2::wrapper::Container::load(&module_library_file_path)
        }; // Load the module.

        match module {
            Ok(module) => {
                println!(
                    "{}",
                    ansi_term::Color::Green.paint(
                        format!(
                            "[MAIN_BACKEND] [INFO] [THREAD {}] [FILE `{}` LINE {}] Successfully loaded module `{}` from `{}`.",
                            std::thread::current().id().as_u64(),
                            file!(),
                            line!(),
                            module_name,
                            module_library_file_path
                        )
                    )
                );

                let result: (
                    tokio::runtime::Runtime,
                    AsyncModifiable<ModuleStatus>,
                ) = module.on_init(module_statuses_by_protocol.clone());
                {
                    let status: AsyncModifiable<ModuleStatus> = result.1.clone();
                    println!(
                        "{}",
                        ansi_term::Color::Blue.paint(
                            format!(
                                "[MAIN_BACKEND] [INFO] [THREAD {}] [FILE `{}` LINE {}] Waiting for module `{}` to finish initialization.",
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!(),
                                module_name
                            )
                        )
                    );
                    // Wait for the module to notify us, instead of polling on a timer. `enable()`
                    // registers us as a waiter immediately, before we check the flag — required
                    // because notify_one() only guarantees delivery to a Notified future that has
                    // already been polled at least once; a bare `notified().await` after the flag
                    // check leaves a real (if narrow) race window on a multi-threaded runtime
                    // where the module's notification can be missed.
                    let init_notify: std::sync::Arc<tokio::sync::Notify> = status
                        .lock().await.init_notify
                        .clone();
                    let notified = init_notify.notified();
                    tokio::pin!(notified);
                    notified.as_mut().enable();
                    let already_initialized: bool = status.lock().await.initialized;
                    if !already_initialized {
                        if tokio::time::timeout(
                            std::time::Duration::from_secs(30),
                            notified.as_mut(),
                        ).await.is_err() {
                            panic!("module `{}` initialization timed out", module_name);
                        }
                    }
                    if status.lock().await.panicked {
                        panic!("module `{}` initialization failed", module_name);
                    }
                    let module_socket_port: u16 = {
                        let guard_status: tokio::sync::MutexGuard<'_, ModuleStatus> =
                            status.lock().await;
                        let port: u16 = *guard_status.socket_port.lock().await;
                        drop(guard_status);
                        port
                    };
                    println!(
                        "{}",
                        ansi_term::Color::Green.paint(
                            format!(
                                "[MAIN_BACKEND] [INFO] [THREAD {}] [FILE `{}` LINE {}] Module `{}` finished initialization, listening on port {}.",
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!(),
                                module_name,
                                module_socket_port
                            )
                        )
                    );
                    let mut guard_module_combinations: tokio::sync::MutexGuard<
                        '_,
                        Vec<ModuleCombination>
                    > = module_combinations.lock().await;
                    guard_module_combinations.push(ModuleCombination {
                        name: String::from(module_name),
                        instance: module,
                        tokio_runtime: result.0,
                    }); // Save the Tokio runtime.
                    drop(guard_module_combinations);
                    let mut guard_module_statuses_by_protocol: tokio::sync::MutexGuard<
                        '_,
                        std::collections::HashMap<String, AsyncModifiable<ModuleStatus>>
                    > = module_statuses_by_protocol.lock().await;
                    guard_module_statuses_by_protocol.insert(
                        String::from(
                            modules_protocol_without_version_by_id.get(&head_ele).unwrap()
                        ),
                        status
                    );
                    drop(guard_module_statuses_by_protocol);
                }

                if let Some(mut tmp) = forward_star_representation.head[head_ele] {
                    loop {
                        let v: usize = forward_star_representation.edges[tmp].to;
                        indegs[v] -= 1;
                        if indegs[v] == 0 {
                            queue.push_back(v);
                        }
                        tmp = forward_star_representation.edges[tmp].nxt;
                        if tmp == 0 {
                            break;
                        }
                    }
                }
            }
            Err(_) => {
                eprintln!(
                    "{}",
                    ansi_term::Color::Red.paint(
                        format!(
                            "[MAIN_BACKEND] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Failed to load module `{}` from `{}`. Maybe the module file doesn't exist or is not a valid shared object?",
                            std::thread::current().id().as_u64(),
                            file!(),
                            line!(),
                            module_name,
                            module_library_file_path
                        )
                    )
                );

                if module_config_json.restricted_mode {
                    eprintln!(
                        "{}",
                        ansi_term::Color::Red.paint(
                            format!(
                                "[MAIN_BACKEND] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Due to the restricted mode, the server backend now is shutting down.",
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!()
                            )
                        )
                    );
                    // TODO: Calling all the other modules to shutdown.
                    panic!();
                }
            }
        }
    }
}

fn unload_module_combinations(
    module_combinations: &mut Vec<ModuleCombination>,
    module_config_json: &ModuleConfigJson
) {
    // Phase 1: ask every module to finish protocol/database cleanup while all runtimes are still
    // alive. This matters for modules that communicate with a service which is not represented as
    // a formal dependency (for example, authenticator sends its unbind message to ws_server).
    let modules: Vec<ModuleCombination> = std::mem::take(module_combinations);
    for module in &modules {
        let unload_timeout: usize = module_config_json.working_load
            .get(&module.name)
            .map(|config| config.unload_timeout)
            .unwrap_or(0);
        module.instance.on_unload(unload_timeout); // Request graceful cleanup.
    }

    // Phase 2: dependencies are loaded before dependants, so stop runtimes in reverse order.
    // Long-lived socket accept loops do not observe a cooperative cancellation signal, so
    // shutdown_timeout would wait the configured 5 seconds for every module. The phase-1 hooks
    // already perform the bounded cleanup; drop each runtime in the background here so process
    // shutdown is not serialized behind those accept loops.
    for module in modules.into_iter().rev() {
        let unload_timeout: usize = module_config_json.working_load
            .get(&module.name)
            .map(|config| config.unload_timeout)
            .unwrap_or(0);
        println!(
            "{}",
            ansi_term::Color::Blue.paint(
                format!(
                    "[MAIN_BACKEND] [INFO] [THREAD {}] [FILE `{}` LINE {}] Releasing the Tokio runtime of module `{}` without waiting (cleanup timeout {}ms).",
                    std::thread::current().id().as_u64(),
                    file!(),
                    line!(),
                    module.name,
                    unload_timeout
                )
            )
        );
        module.tokio_runtime.shutdown_background();
    }
}
