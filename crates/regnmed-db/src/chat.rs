//! Meldinger: one-to-one conversations (docs/meldinger.md, migration 0091).
//!
//! **Who may talk to whom is computed, never stored** ([`contacts`]):
//!
//! - a company's people reach their colleagues, their company's agents,
//!   and the contact person(s) of each byrå with an active engagement;
//! - a byrå's people reach their colleagues, and every person and agent
//!   in each client company.
//!
//! Reading needs the same reach as writing. When an engagement or a
//! membership ends, the conversation leaves both lists at once -- the
//! rows stay (they are part of the trail), the access does not.
//!
//! Writing to an agent is giving it an order: the message creates an
//! `agent_oppgave` (#103), or continues the agent's open question, and
//! the agent's answer comes back through [`mirror_task_answer`]. The
//! agent still acts only with its own role's authority.

use std::collections::HashMap;

use anyhow::{Result, bail, ensure};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

/// How a contact is reached. Several when the same person is reachable
/// more than one way (a colleague in two companies).
#[derive(Debug, Clone)]
pub struct Link {
    /// `colleague`, `agent`, `accountant` (the byrå's contact),
    /// `firm_colleague` or `client`.
    pub via: String,
    pub company_id: Option<Uuid>,
    pub company_name: Option<String>,
    pub firm_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Contact {
    pub person_id: Uuid,
    pub name: String,
    pub agent: bool,
    /// An agent is one contact PER COMPANY: its tasks go to one ledger.
    pub company_id: Option<Uuid>,
    pub avatar_color: Option<String>,
    pub avatar_icon: Option<String>,
    pub links: Vec<Link>,
}

impl Contact {
    /// The companies an attachment to this person can go to: one both
    /// sides work in. A byrå colleague shares none.
    pub fn companies(&self) -> Vec<Uuid> {
        let mut out: Vec<Uuid> = self.links.iter().filter_map(|l| l.company_id).collect();
        out.sort();
        out.dedup();
        out
    }
}

/// Everybody `me` may talk to right now.
pub async fn contacts(pool: &PgPool, me: Uuid) -> Result<Vec<Contact>> {
    let rows = sqlx::query(
        "with mc as (select company_id from company_member where person_id = $1 and active),
              mf as (select firm_id from firm_member where person_id = $1 and active),
              eng as (select id, firm_id, company_id from engagement
                      where valid_from <= current_date
                        and (valid_to is null or valid_to > current_date)),
              agents as (select g.company_id, i.person_id
                         from integration_grant g
                         join integration i on i.id = g.integration_id
                         where g.valid_from <= current_date
                           and (g.valid_to is null or g.valid_to > current_date)),
              reach as (
                  select cm.person_id, 'colleague' as via, cm.company_id, null::uuid as firm_id
                  from company_member cm join mc using (company_id)
                  where cm.active
                  union all
                  select a.person_id, 'agent', a.company_id, null
                  from agents a join mc using (company_id)
                  union all
                  -- The byrå's chosen contacts, or its admins until it chose.
                  select fm.person_id, 'accountant', e.company_id, e.firm_id
                  from eng e join mc using (company_id)
                  join firm_member fm on fm.firm_id = e.firm_id and fm.active
                  left join engagement_contact ec on ec.engagement_id = e.id
                  -- A chosen contact who has left the byrå counts as none chosen.
                  where case when exists (select 1 from firm_member x
                                          where x.firm_id = e.firm_id and x.active
                                            and x.person_id = any (ec.person_ids))
                             then fm.person_id = any (ec.person_ids)
                             else fm.role = 'admin' end
                  union all
                  select fm.person_id, 'firm_colleague', null, fm.firm_id
                  from firm_member fm join mf using (firm_id)
                  where fm.active
                  union all
                  select cm.person_id, 'client', e.company_id, e.firm_id
                  from eng e join mf using (firm_id)
                  join company_member cm on cm.company_id = e.company_id and cm.active
                  union all
                  select a.person_id, 'agent', e.company_id, e.firm_id
                  from eng e join mf using (firm_id)
                  join agents a on a.company_id = e.company_id
              )
         select r.person_id, r.via, r.company_id, c.name as company_name, f.name as firm_name,
                coalesce(i.navn, p.name, p.oidc_sub) as name, i.id is not null as agent,
                i.avatar_farge, i.avatar_ikon
         from reach r
         join person p on p.id = r.person_id
         left join integration i on i.person_id = r.person_id
         left join company c on c.id = r.company_id
         left join firm f on f.id = r.firm_id
         where r.person_id <> $1
         order by 6, c.name nulls first",
    )
    .bind(me)
    .fetch_all(pool)
    .await?;

    let mut out: Vec<Contact> = Vec::new();
    let mut index: HashMap<(Uuid, Option<Uuid>), usize> = HashMap::new();
    for r in &rows {
        let agent: bool = r.get("agent");
        let person_id: Uuid = r.get("person_id");
        let company_id: Option<Uuid> = r.get("company_id");
        let key = (person_id, if agent { company_id } else { None });
        let link = Link {
            via: r.get("via"),
            company_id,
            company_name: r.get("company_name"),
            firm_name: r.get("firm_name"),
        };
        match index.get(&key) {
            Some(&i) => {
                let links = &mut out[i].links;
                if !links
                    .iter()
                    .any(|l| l.via == link.via && l.company_id == link.company_id)
                {
                    links.push(link);
                }
            }
            None => {
                index.insert(key, out.len());
                out.push(Contact {
                    person_id,
                    name: r.get("name"),
                    agent,
                    company_id: key.1,
                    avatar_color: r.get("avatar_farge"),
                    avatar_icon: r.get("avatar_ikon"),
                    links: vec![link],
                });
            }
        }
    }
    Ok(out)
}

/// The contact `person` is to `me`, if `me` may talk to them now.
pub fn find(contacts: &[Contact], person: Uuid, company: Option<Uuid>) -> Option<&Contact> {
    contacts
        .iter()
        .find(|c| c.person_id == person && (!c.agent || c.company_id == company))
}

/// One conversation as `me` sees it.
#[derive(Debug, Clone)]
pub struct Conversation {
    pub id: Uuid,
    pub company_id: Option<Uuid>,
    pub other: Uuid,
    pub created_at: DateTime<Utc>,
    pub last_text: Option<String>,
    pub last_author: Option<Uuid>,
    pub last_at: Option<DateTime<Utc>>,
    pub unread: i64,
}

/// `me`'s conversations, the one with the newest message first. Includes
/// conversations `me` can no longer reach; the caller filters them out
/// against [`contacts`] (one query for both, instead of one per row).
pub async fn conversations(pool: &PgPool, me: Uuid) -> Result<Vec<Conversation>> {
    let rows = sqlx::query(
        "select c.id, c.company_id, c.created_at, other.person_id as other,
                lm.body, lm.author_person_id, lm.created_at as last_at,
                (select count(*) from chat_message m
                 where m.conversation_id = c.id and m.author_person_id <> $1
                   and m.created_at > coalesce(mine.read_until, '-infinity')) as unread
         from chat_participant mine
         join chat_conversation c on c.id = mine.conversation_id
         join chat_participant other on other.conversation_id = c.id and other.person_id <> $1
         left join lateral (
             select body, author_person_id, created_at from chat_message
             where conversation_id = c.id order by created_at desc, id desc limit 1
         ) lm on true
         where mine.person_id = $1
         order by coalesce(lm.created_at, c.created_at) desc",
    )
    .bind(me)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Conversation {
            id: r.get("id"),
            company_id: r.get("company_id"),
            other: r.get("other"),
            created_at: r.get("created_at"),
            last_text: r.get("body"),
            last_author: r.get("author_person_id"),
            last_at: r.get("last_at"),
            unread: r.get("unread"),
        })
        .collect())
}

/// The one conversation between `me` and `other` (in `company`, for an
/// agent), created on first use. The caller has checked the reach.
pub async fn open(pool: &PgPool, me: Uuid, other: Uuid, company: Option<Uuid>) -> Result<Uuid> {
    ensure!(me != other, "du kan ikke starte en samtale med deg selv");
    let (a, b) = if me < other { (me, other) } else { (other, me) };
    let key = match company {
        Some(c) => format!("{a}:{b}:{c}"),
        None => format!("{a}:{b}"),
    };
    let mut tx = pool.begin().await?;
    sqlx::query(
        "insert into chat_conversation (id, company_id, direct_key) values ($1, $2, $3)
         on conflict (direct_key) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(company)
    .bind(&key)
    .execute(&mut *tx)
    .await?;
    let id: Uuid = sqlx::query_scalar("select id from chat_conversation where direct_key = $1")
        .bind(&key)
        .fetch_one(&mut *tx)
        .await?;
    for person in [a, b] {
        sqlx::query(
            "insert into chat_participant (conversation_id, person_id) values ($1, $2)
             on conflict do nothing",
        )
        .bind(id)
        .bind(person)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(id)
}

/// The conversation if `me` is in it: `(company_id, the other person)`.
pub async fn membership(pool: &PgPool, me: Uuid, id: Uuid) -> Result<Option<(Option<Uuid>, Uuid)>> {
    let row = sqlx::query(
        "select c.company_id, other.person_id
         from chat_participant mine
         join chat_conversation c on c.id = mine.conversation_id
         join chat_participant other on other.conversation_id = c.id and other.person_id <> $1
         where mine.person_id = $1 and c.id = $2",
    )
    .bind(me)
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get(0), r.get(1))))
}

#[derive(Debug, Clone)]
pub struct Message {
    pub id: Uuid,
    pub author_person_id: Uuid,
    pub author_name: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    /// `[{document_id, company_id, filename, content_type}]`
    pub documents: serde_json::Value,
    pub task_id: Option<Uuid>,
    /// The task's newest status, so the thread shows what the agent is
    /// doing with the order.
    pub task_status: Option<String>,
}

/// The newest 200 messages, or the first 200 after `since`; oldest first.
///
/// `since` is honoured with five seconds of overlap: `created_at` is the
/// transaction's start, so a message can commit after a later-stamped one
/// and would otherwise be skipped for good. The client de-duplicates by
/// id, which is cheaper than a sequence every writer must lock.
pub async fn messages(
    pool: &PgPool,
    conversation: Uuid,
    since: Option<DateTime<Utc>>,
) -> Result<Vec<Message>> {
    let rows = sqlx::query(
        "select m.id, m.author_person_id, m.author_name, m.body, m.created_at, m.task_id,
                case when m.task_id is null then null else
                    coalesce((select h.status from agent_oppgave_hendelse h
                              where h.oppgave_id = m.task_id
                              order by h.created_at desc, h.id desc limit 1), 'ny') end
                    as task_status,
                coalesce((select json_agg(json_build_object(
                              'document_id', d.id, 'company_id', d.company_id,
                              'filename', d.filename, 'content_type', d.content_type)
                              order by d.filename)
                          from chat_message_document cd
                          join inbox_document d on d.id = cd.document_id
                          where cd.message_id = m.id), '[]')::text as documents
         from chat_message m
         where m.conversation_id = $1
           and ($2::timestamptz is null or m.created_at > $2 - interval '5 seconds')
         -- The newest 200 on a full read; with `since`, the OLDEST 200 after
         -- it, so a burst longer than a page is continued by the next call
         -- instead of leaving a hole in the middle.
         order by case when $2::timestamptz is null then m.created_at end desc,
                  case when $2::timestamptz is null then m.id end desc,
                  m.created_at, m.id
         limit 200",
    )
    .bind(conversation)
    .bind(since)
    .fetch_all(pool)
    .await?;
    let mut out: Vec<Message> = rows
        .iter()
        .map(|r| {
            Ok(Message {
                id: r.get("id"),
                author_person_id: r.get("author_person_id"),
                author_name: r.get("author_name"),
                body: r.get("body"),
                created_at: r.get("created_at"),
                documents: serde_json::from_str(r.get("documents"))?,
                task_id: r.get("task_id"),
                task_status: r.get("task_status"),
            })
        })
        .collect::<Result<_>>()?;
    if since.is_none() {
        out.reverse();
    }
    Ok(out)
}

pub struct Author<'a> {
    pub person_id: Uuid,
    pub name: &'a str,
}

/// The agent on the other side of a conversation, when there is one.
pub struct AgentTarget {
    pub person_id: Uuid,
    pub company_id: Uuid,
}

/// Who to ring about a new message: everyone in the conversation.
#[derive(Debug, Clone)]
pub struct Announcement {
    pub conversation_id: Uuid,
    pub message_id: Uuid,
    pub person_ids: Vec<Uuid>,
}

/// Writes a message and its attachments in ONE transaction, and -- when
/// the other side is an agent -- the order it gives. A message that names
/// a task that does not exist, or the other way round, would be a trail
/// that lies. Returns the announcement and the task to ring the queue for.
///
/// `attachable` is the companies both sides work in: an attachment is a
/// document in one company's innboks, and must stay among its people.
pub async fn post(
    pool: &PgPool,
    conversation: Uuid,
    author: &Author<'_>,
    body: &str,
    documents: &[Uuid],
    attachable: &[Uuid],
    agent: Option<&AgentTarget>,
) -> Result<(Announcement, Option<Uuid>)> {
    let body = body.trim();
    ensure!(
        !body.is_empty() || !documents.is_empty(),
        "meldingen er tom"
    );
    // An attachment alone still gets a line of text, so the thread never
    // shows an empty bubble and the agent always has an order to read.
    let body = if body.is_empty() { "(vedlegg)" } else { body };
    ensure!(
        body.chars().count() <= 8000,
        "meldingen er for lang (maks 8000 tegn)"
    );

    // The same file attached twice is one document (the inbox links a
    // copy already there), and the agent's order must name it once.
    let mut unique: Vec<Uuid> = Vec::new();
    for doc in documents {
        if !unique.contains(doc) {
            unique.push(*doc);
        }
    }
    let documents = unique.as_slice();

    let mut tx = pool.begin().await?;
    let mut files: Vec<String> = Vec::new();
    for doc in documents {
        let row = sqlx::query("select filename, company_id from inbox_document where id = $1")
            .bind(doc)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            bail!("vedlegget finnes ikke");
        };
        let company: Uuid = row.get("company_id");
        ensure!(
            attachable.contains(&company),
            "vedlegget ligger i et selskap dere ikke deler"
        );
        files.push(format!(
            "innboksdokument {doc} («{}»)",
            row.get::<String, _>("filename")
        ));
    }

    let task_id = match agent {
        Some(target) => {
            Some(order_agent(&mut tx, conversation, target, author, body, &files).await?)
        }
        None => None,
    };
    let message_id = Uuid::now_v7();
    sqlx::query(
        "insert into chat_message
             (id, conversation_id, author_person_id, author_name, body, task_id)
         values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(message_id)
    .bind(conversation)
    .bind(author.person_id)
    .bind(author.name)
    .bind(body)
    .bind(task_id)
    .execute(&mut *tx)
    .await?;
    for doc in documents {
        sqlx::query(
            "insert into chat_message_document (message_id, document_id) values ($1, $2)
             on conflict do nothing",
        )
        .bind(message_id)
        .bind(doc)
        .execute(&mut *tx)
        .await?;
    }
    // What you just wrote is read.
    sqlx::query(
        "update chat_participant
            set read_until = greatest(coalesce(read_until, '-infinity'), now())
          where conversation_id = $1 and person_id = $2",
    )
    .bind(conversation)
    .bind(author.person_id)
    .execute(&mut *tx)
    .await?;
    let person_ids = participants_in(&mut tx, conversation).await?;
    tx.commit().await?;
    Ok((
        Announcement {
            conversation_id: conversation,
            message_id,
            person_ids,
        },
        task_id,
    ))
}

async fn participants_in(
    tx: &mut Transaction<'_, Postgres>,
    conversation: Uuid,
) -> Result<Vec<Uuid>> {
    Ok(
        sqlx::query_scalar("select person_id from chat_participant where conversation_id = $1")
            .bind(conversation)
            .fetch_all(&mut **tx)
            .await?,
    )
}

/// Turns a message to an agent into work for it.
///
/// If the agent's newest order in this conversation is waiting on a
/// question (`sporsmal`), the message ANSWERS it: a `pagar` event with
/// the text, exactly what a reply in the task thread writes, so the agent
/// resumes with its whole history. Otherwise it is a new order, carrying
/// the recent conversation because the agent reads nothing else.
async fn order_agent(
    tx: &mut Transaction<'_, Postgres>,
    conversation: Uuid,
    target: &AgentTarget,
    author: &Author<'_>,
    body: &str,
    files: &[String],
) -> Result<Uuid> {
    let mut text = body.to_string();
    if !files.is_empty() {
        text.push_str("\n\nVedlegg (ligger i bilagsinnboksen):\n- ");
        text.push_str(&files.join("\n- "));
    }

    let open: Option<(Uuid, Option<String>)> = sqlx::query_as(
        "select m.task_id,
                (select h.status from agent_oppgave_hendelse h where h.oppgave_id = m.task_id
                 order by h.created_at desc, h.id desc limit 1)
         from chat_message m
         where m.conversation_id = $1 and m.task_id is not null
         order by m.created_at desc, m.id desc
         limit 1",
    )
    .bind(conversation)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((task_id, Some(status))) = open
        && status == "sporsmal"
    {
        sqlx::query(
            "insert into agent_oppgave_hendelse (id, oppgave_id, status, notat, av)
             values ($1, $2, 'pagar', $3, $4)",
        )
        .bind(Uuid::now_v7())
        .bind(task_id)
        .bind(&text)
        .bind(author.name)
        .execute(&mut **tx)
        .await?;
        return Ok(task_id);
    }

    // The conversation so far, oldest first. Ten lines is enough to know
    // what "denne" refers to, and short enough for a small model.
    let context: Vec<(String, String)> = sqlx::query_as(
        "select author_name, body from (
             select author_name, body, created_at, id from chat_message
             where conversation_id = $1 order by created_at desc, id desc limit 10
         ) t order by created_at, id",
    )
    .bind(conversation)
    .fetch_all(&mut **tx)
    .await?;
    let mut order = format!(
        "{} skrev til deg i meldinger:\n\n{text}\n\nSvaret ditt legges i samtalen med {}.",
        author.name, author.name
    );
    if !context.is_empty() {
        order.push_str("\n\nTidligere i samtalen:\n");
        order.push_str(
            &context
                .iter()
                .map(|(who, said)| format!("{who}: {said}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }

    let integration_id: Uuid =
        sqlx::query_scalar("select id from integration where person_id = $1")
            .bind(target.person_id)
            .fetch_one(&mut **tx)
            .await?;
    let task_id = Uuid::now_v7();
    // The same row agent_task::create_task writes; inline because it must
    // be in this transaction, and the caller's reach check already proved
    // the grant is active.
    sqlx::query(
        "insert into agent_oppgave (id, company_id, integration_id, melding, opprettet_av)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(task_id)
    .bind(target.company_id)
    .bind(integration_id)
    .bind(order)
    .bind(author.name)
    .execute(&mut **tx)
    .await?;
    Ok(task_id)
}

/// Puts an agent's answer to an order from a conversation back into it.
///
/// None when the task did not come from a conversation, the event is not
/// something anybody said (a bare `pagar`), or `speaker` is not the
/// integration the task was given to. A human closing the order in the
/// task view never gets here (`agent_task::answer`), so it cannot speak
/// as the agent -- decided by identity, not by a name anyone can share.
pub async fn mirror_task_answer(
    pool: &PgPool,
    task_id: Uuid,
    status: &str,
    note: Option<&str>,
    speaker: Uuid,
) -> Result<Option<Announcement>> {
    if status == "pagar" {
        return Ok(None);
    }
    let origin = sqlx::query(
        "select m.conversation_id, i.person_id as agent_person_id, i.navn
         from chat_message m
         join agent_oppgave o on o.id = m.task_id
         join integration i on i.id = o.integration_id
         where m.task_id = $1 and o.integration_id = $2
         order by m.created_at desc, m.id desc
         limit 1",
    )
    .bind(task_id)
    .bind(speaker)
    .fetch_optional(pool)
    .await?;
    let Some(origin) = origin else {
        return Ok(None);
    };
    let conversation: Uuid = origin.get("conversation_id");
    // At most 8000 characters a message (0091); a longer report is cut
    // rather than lost, and the full note stays in the task.
    let body: String = match note.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => n.chars().take(8000).collect(),
        None => "Ferdig.".to_string(),
    };

    let mut tx = pool.begin().await?;
    let message_id = Uuid::now_v7();
    sqlx::query(
        "insert into chat_message (id, conversation_id, author_person_id, author_name, body)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(message_id)
    .bind(conversation)
    .bind(origin.get::<Uuid, _>("agent_person_id"))
    .bind(origin.get::<String, _>("navn"))
    .bind(&body)
    .execute(&mut *tx)
    .await?;
    let person_ids = participants_in(&mut tx, conversation).await?;
    tx.commit().await?;
    Ok(Some(Announcement {
        conversation_id: conversation,
        message_id,
        person_ids,
    }))
}

/// Moves the reader's mark forward. Never backwards: two tabs reading at
/// different speeds must not resurrect messages as unread.
pub async fn mark_read(
    pool: &PgPool,
    conversation: Uuid,
    me: Uuid,
    until: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "update chat_participant
            set read_until = greatest(coalesce(read_until, '-infinity'), $3)
          where conversation_id = $1 and person_id = $2",
    )
    .bind(conversation)
    .bind(me)
    .bind(until)
    .execute(pool)
    .await?;
    Ok(())
}

/// The byrå's chosen contacts per engagement: `engagement_id -> persons`.
pub async fn firm_contacts(pool: &PgPool, firm: Uuid) -> Result<HashMap<Uuid, Vec<Uuid>>> {
    let rows: Vec<(Uuid, Vec<Uuid>)> = sqlx::query_as(
        "select ec.engagement_id, ec.person_ids
         from engagement_contact ec join engagement e on e.id = ec.engagement_id
         where e.firm_id = $1",
    )
    .bind(firm)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Chooses who a client writes to. Every person must be an active member
/// of the byrå; an empty list means "the admins" again.
pub async fn set_firm_contacts(
    pool: &PgPool,
    firm: Uuid,
    engagement: Uuid,
    persons: &[Uuid],
    by: &str,
) -> Result<()> {
    let owned: bool = sqlx::query_scalar(
        "select exists (select 1 from engagement where id = $1 and firm_id = $2)",
    )
    .bind(engagement)
    .bind(firm)
    .fetch_one(pool)
    .await?;
    ensure!(owned, "oppdraget tilhører ikke byrået");
    let mut unique = persons.to_vec();
    unique.sort();
    unique.dedup();
    let members: i64 = sqlx::query_scalar(
        "select count(*) from firm_member where firm_id = $1 and active and person_id = any ($2)",
    )
    .bind(firm)
    .bind(&unique)
    .fetch_one(pool)
    .await?;
    ensure!(
        members as usize == unique.len(),
        "en kontaktperson må være aktivt medlem av byrået"
    );
    sqlx::query(
        "insert into engagement_contact (engagement_id, person_ids, set_by) values ($1, $2, $3)
         on conflict (engagement_id) do update
            set person_ids = excluded.person_ids, set_by = excluded.set_by, set_at = now()",
    )
    .bind(engagement)
    .bind(&unique)
    .bind(by)
    .execute(pool)
    .await?;
    Ok(())
}
