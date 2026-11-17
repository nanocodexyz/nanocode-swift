use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap},
    ffi::OsStr,
    fs::{DirBuilder, OpenOptions},
    io::{self, Write as _},
    os::unix::{
        ffi::OsStrExt as _,
        fs::{DirBuilderExt as _, MetadataExt as _},
    },
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

use nanocode_python::{
    Child, Ended, Exited, Files, Kind, Limit, Limits, Python, Remove, Run, Stat, Stdio, Write,
};
use serde::Deserialize;

use super::{Message, STACK, STOPPED};
use crate::{files::local, session::text};

pub const ROOT: &str = "/usr/local/lib/python3.14";

#[derive(Deserialize)]
struct Request {
    argv: Vec<String>,
    cwd: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
    output: u64,
    file: u64,
}

pub struct Ran {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

struct Job {
    id: u64,
    request: Request,
    stdin: Vec<u8>,
    root: PathBuf,
    stop: Arc<AtomicBool>,
    sender: mpsc::Sender<Message>,
}

pub struct Pythons {
    root: PathBuf,
    sender: mpsc::Sender<Message>,
    stops: RefCell<HashMap<u64, Arc<AtomicBool>>>,
    children: RefCell<HashMap<u64, mpsc::Sender<io::Result<Exited>>>>,
    next: Cell<u64>,
}

impl Pythons {
    pub fn new(root: PathBuf, sender: mpsc::Sender<Message>) -> Self {
        Self {
            root,
            sender,
            stops: RefCell::default(),
            children: RefCell::default(),
            next: Cell::new(1),
        }
    }

    pub fn start(&self, id: u64, request: &str, stdin: Vec<u8>) {
        let request = match serde_json::from_str(request) {
            Ok(request) => request,
            Err(error) => {
                let _ = self.sender.send(Message::Python {
                    id,
                    result: Err(error.to_string()),
                });
                return;
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        self.stops.borrow_mut().insert(id, Arc::clone(&stop));
        let job = Job {
            id,
            request,
            stdin,
            root: self.root.clone(),
            stop,
            sender: self.sender.clone(),
        };
        if let Err(error) = submit(job) {
            let _ = self.sender.send(Message::Python {
                id,
                result: Err(error),
            });
        }
    }

    pub fn stop(&self, id: u64) {
        if let Some(stop) = self.stops.borrow().get(&id) {
            stop.store(true, Ordering::Relaxed);
        }
    }

    pub fn finished(&self, id: u64) {
        self.stops.borrow_mut().remove(&id);
    }

    pub fn child(&self, reply: mpsc::Sender<io::Result<Exited>>) -> u64 {
        let token = self.next.get();
        self.next.set(token + 1);
        self.children.borrow_mut().insert(token, reply);
        token
    }

    pub fn exited(&self, token: u64, exited: io::Result<Exited>) {
        if let Some(reply) = self.children.borrow_mut().remove(&token) {
            let _ = reply.send(exited);
        }
    }
}

impl Drop for Pythons {
    fn drop(&mut self) {
        for stop in self.stops.get_mut().values() {
            stop.store(true, Ordering::Relaxed);
        }
        for (_, reply) in self.children.get_mut().drain() {
            let _ = reply.send(Err(io::Error::other(STOPPED)));
        }
    }
}

fn submit(job: Job) -> Result<(), String> {
    static WORKER: LazyLock<Result<mpsc::Sender<Job>, String>> = LazyLock::new(|| {
        let (queue, jobs) = mpsc::channel();
        std::thread::Builder::new()
            .name("nanocode-python".to_owned())
            .stack_size(STACK)
            .spawn(move || serve(&jobs))
            .map_err(|error| format!("start Python: {error}"))?;
        Ok(queue)
    });
    WORKER
        .as_ref()
        .map_err(Clone::clone)?
        .send(job)
        .map_err(|_| "Python stopped unexpectedly".to_owned())
}

fn serve(jobs: &mpsc::Receiver<Job>) {
    let mut python = None;
    while let Ok(job) = jobs.recv() {
        let (id, sender) = (job.id, job.sender.clone());
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| run(&mut python, job)))
            .unwrap_or_else(|_| {
                python = None;
                Err("Python stopped unexpectedly".to_owned())
            });
        let _ = sender.send(Message::Python { id, result });
    }
}

fn run(python: &mut Option<Python>, job: Job) -> Result<Ran, String> {
    let Job {
        id,
        request,
        stdin,
        root,
        stop,
        sender,
    } = job;
    if stop.load(Ordering::Relaxed) {
        return Ok(Ran {
            status: 130,
            stdout: Vec::new(),
            stderr: Vec::new(),
        });
    }
    let python = match python {
        Some(python) => python,
        None => python.insert(Python::new().map_err(|error| text(&error))?),
    };
    let run = Run {
        argv: request.argv,
        stdio: Stdio::Held(stdin),
        cwd: request.cwd.into_bytes(),
        env: request
            .env
            .into_iter()
            .map(|(name, value)| (name.into_bytes(), value.into_bytes()))
            .collect(),
        limits: Limits {
            output: request.output,
            file: request.file,
            sleep: Duration::MAX,
            steps: u64::MAX,
            time: Duration::MAX,
            memory: None,
            depth: usize::MAX,
        },
        stop: Arc::new(move || stop.load(Ordering::Relaxed)),
    };
    let ran = python.run(run, Rc::new(Workspace { root, sender, id }));
    let mut stderr = ran.stderr;
    let status = match ran.ended {
        Ended::Exited(code) => code,
        Ended::Needs(missing) => {
            for missing in missing {
                let _ = writeln!(stderr, "{missing}");
            }
            127
        }
        Ended::Refused(limit) => {
            let why = match limit {
                Limit::Output => "it printed more than this shell keeps",
                Limit::Sleep => "it cannot wait in this shell",
                Limit::Steps => "it ran more steps than this shell allows",
                Limit::Time => "it ran longer than this shell allows",
                Limit::Memory => "it held more memory than this shell allows",
            };
            let _ = writeln!(stderr, "python3: {why}");
            126
        }
        Ended::Cancelled => 130,
    };
    Ok(Ran {
        status,
        stdout: ran.stdout,
        stderr,
    })
}

pub fn real(root: &Path, path: &Path) -> PathBuf {
    match path.strip_prefix(ROOT) {
        Ok(rest) if rest.as_os_str().is_empty() => root.to_path_buf(),
        Ok(rest) => root.join(rest),
        Err(_) => local(path),
    }
}

struct Workspace {
    root: PathBuf,
    sender: mpsc::Sender<Message>,
    id: u64,
}

impl Workspace {
    fn real(&self, path: &[u8]) -> PathBuf {
        real(&self.root, Path::new(OsStr::from_bytes(path)))
    }
}

impl Files for Workspace {
    fn stat(&self, path: &[u8]) -> io::Result<Stat> {
        let metadata = std::fs::metadata(self.real(path))?;
        Ok(Stat {
            kind: if metadata.is_dir() {
                Kind::Dir
            } else {
                Kind::File
            },
            size: if metadata.is_file() {
                metadata.len()
            } else {
                0
            },
            mode: metadata.mode() & 0o7777,
            mtime_ns: metadata
                .mtime()
                .saturating_mul(1_000_000_000)
                .saturating_add(metadata.mtime_nsec()),
        })
    }

    fn read(&self, path: &[u8], limit: u64) -> io::Result<Vec<u8>> {
        let path = self.real(path);
        let metadata = std::fs::metadata(&path)?;
        if metadata.is_dir() {
            return Err(io::ErrorKind::IsADirectory.into());
        }
        if metadata.len() > limit {
            return Err(io::ErrorKind::FileTooLarge.into());
        }
        if !metadata.is_file() {
            return Ok(Vec::new());
        }
        std::fs::read(path)
    }

    fn write(&self, path: &[u8], data: &[u8], how: Write) -> io::Result<()> {
        let mut options = OpenOptions::new();
        match how {
            Write::Replace => options.create(true).write(true).truncate(true),
            Write::Append => options.create(true).append(true),
            Write::New => options.create_new(true).write(true),
        };
        options.open(self.real(path))?.write_all(data)
    }

    fn list(&self, path: &[u8]) -> io::Result<Vec<Vec<u8>>> {
        let mut names = std::fs::read_dir(self.real(path))?
            .map(|entry| entry.map(|entry| entry.file_name().as_bytes().to_vec()))
            .collect::<io::Result<Vec<_>>>()?;
        names.sort();
        Ok(names)
    }

    fn mkdir(&self, path: &[u8], mode: u32) -> io::Result<()> {
        DirBuilder::new().mode(mode).create(self.real(path))
    }

    fn remove(&self, path: &[u8], what: Remove) -> io::Result<()> {
        match what {
            Remove::File => std::fs::remove_file(self.real(path)),
            Remove::Dir => std::fs::remove_dir(self.real(path)),
        }
    }

    fn rename(&self, from: &[u8], to: &[u8]) -> io::Result<()> {
        std::fs::rename(self.real(from), self.real(to))
    }

    fn spawns(&self) -> bool {
        true
    }

    fn spawn(&self, child: Child) -> io::Result<Exited> {
        let (reply, exited) = mpsc::channel();
        self.sender
            .send(Message::Spawn {
                id: self.id,
                child,
                reply,
            })
            .map_err(|_| io::Error::other(STOPPED))?;
        exited.recv().map_err(|_| io::Error::other(STOPPED))?
    }
}
