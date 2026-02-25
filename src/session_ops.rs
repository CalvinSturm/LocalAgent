use anyhow::anyhow;

use crate::session::SessionStore;
use crate::{SessionMemorySubcommand, SessionSubcommand};

pub(crate) fn handle_session_command(
    store: &SessionStore,
    cmd: &SessionSubcommand,
) -> anyhow::Result<()> {
    match cmd {
        SessionSubcommand::Info => {
            let data = store.load()?;
            println!(
                "session={} messages={} memory={} updated_at={}",
                data.name,
                data.messages.len(),
                data.task_memory.len(),
                data.updated_at
            );
        }
        SessionSubcommand::Show { last } => {
            let data = store.load()?;
            let len = data.messages.len();
            let start = len.saturating_sub(*last);
            for (idx, m) in data.messages.iter().enumerate().skip(start) {
                let role = format!("{:?}", m.role).to_uppercase();
                println!(
                    "{} {}: {}",
                    idx,
                    role,
                    m.content.clone().unwrap_or_default().replace('\n', " ")
                );
            }
        }
        SessionSubcommand::Drop { from, last } => match (from, last) {
            (Some(i), None) => {
                store.drop_from(*i)?;
                println!("dropped messages from index {}", i);
            }
            (None, Some(n)) => {
                store.drop_last(*n)?;
                println!("dropped last {} messages", n);
            }
            _ => return Err(anyhow!("provide exactly one of --from or --last")),
        },
        SessionSubcommand::Reset => {
            store.reset()?;
            println!("session reset");
        }
        SessionSubcommand::Memory { command } => match command {
            SessionMemorySubcommand::Add { title, content } => {
                let id = store.add_memory(title, content)?;
                println!("added memory {}", id);
            }
            SessionMemorySubcommand::List => {
                let data = store.load()?;
                let mut blocks = data.task_memory.clone();
                blocks.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
                for b in blocks {
                    println!("{}\t{}\t{}", b.id, b.title, b.updated_at);
                }
            }
            SessionMemorySubcommand::Show { id } => {
                let data = store.load()?;
                let Some(b) = data.task_memory.iter().find(|m| m.id == *id) else {
                    return Err(anyhow!("memory id not found: {}", id));
                };
                println!(
                    "id={}\ntitle={}\ncreated_at={}\nupdated_at={}\ncontent={}",
                    b.id, b.title, b.created_at, b.updated_at, b.content
                );
            }
            SessionMemorySubcommand::Update { id, title, content } => {
                store.update_memory(id, title.as_deref(), content.as_deref())?;
                println!("updated memory {}", id);
            }
            SessionMemorySubcommand::Delete { id } => {
                store.delete_memory(id)?;
                println!("deleted memory {}", id);
            }
        },
    }
    Ok(())
}
