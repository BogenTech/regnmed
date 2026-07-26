-- Aksjeeierbok og aksjonærregisteroppgave (docs/aksjonaer.md, #43).
--
-- Aksjeeierboken er et LOVPÅLAGT REGISTER i seg selv: aksjeloven §4-5
-- pålegger styret å opprette og føre den, den skal føres på betryggende
-- måte, og den kan føres elektronisk. Den har verdi lenge før noen
-- leverer en oppgave til Skatteetaten — og fra juni 2026 er et
-- sluttbrukersystem den ENESTE veien til å levere
-- aksjonærregisteroppgaven, siden Altinn.no og papir er avviklet.
--
-- Modellen følger hovedbokens filosofi: eierandelen LAGRES ALDRI. En
-- «antall aksjer»-kolonne noen overskriver er en påstand; en
-- innsettings-bare hendelsesrekke er et bevis, og gir aksjeeierboken på
-- en hvilken som helst dato gratis.
--
-- PERSONVERN, bevisst: oppgaven krever fødselsnummer for personlige
-- aksjonærer, men aksjeeierboken etter §4-5 krever bare FØDSELSDATO — og
-- boken er et register enhver har innsynsrett i. Derfor lagres
-- fødselsnummeret her fordi innrapporteringen trenger det, mens
-- aksjeeierbok-visningen utleder fødselsdatoen fra det
-- (regnmed-core::fnr) og aldri viser nummeret.

create table shareholder (
    id           uuid primary key,
    company_id   uuid not null references company (id),
    -- Identiteten er permanent: den er det oppgaven rapporterer under,
    -- og et bytte er en ny aksjonær, ikke en redigering.
    kind         text not null check (kind in ('person', 'selskap', 'utenlandsk')),
    fodselsnummer text check (fodselsnummer ~ '^[0-9]{11}$'),
    orgnr        text check (orgnr ~ '^[0-9]{9}$'),
    -- Aksjonær-ID tildelt av Aksjonærregisteret: UTL + 9 siffer.
    utenlandsk_id text check (utenlandsk_id ~ '^UTL[0-9]{9}$'),
    -- Kontaktopplysninger er redigerbare — folk flytter.
    navn         text not null check (navn <> ''),
    adresse      text,
    postnummer   text,
    poststed     text,
    landkode     text check (landkode is null or landkode ~ '^[A-Z]{2}$'),
    note         text,
    created_by   text not null check (created_by <> ''),
    created_at   timestamptz not null default now(),
    -- Hver art har sin identifikator, og bare sin.
    check (case kind
               when 'person' then fodselsnummer is not null and orgnr is null and utenlandsk_id is null
               when 'selskap' then orgnr is not null and fodselsnummer is null and utenlandsk_id is null
               else utenlandsk_id is not null and fodselsnummer is null and orgnr is null
           end)
);

create index shareholder_company_idx on shareholder (company_id, navn);
-- Samme person kan ikke stå to ganger i samme aksjeeierbok.
create unique index shareholder_fnr_idx on shareholder (company_id, fodselsnummer)
    where fodselsnummer is not null;
create unique index shareholder_orgnr_idx on shareholder (company_id, orgnr)
    where orgnr is not null;
create unique index shareholder_utl_idx on shareholder (company_id, utenlandsk_id)
    where utenlandsk_id is not null;

create function shareholder_guard() returns trigger
language plpgsql as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'aksjonærer slettes ikke — eierhistorikken skal stå';
    end if;
    if new.id is distinct from old.id
       or new.company_id is distinct from old.company_id
       or new.kind is distinct from old.kind
       or new.fodselsnummer is distinct from old.fodselsnummer
       or new.orgnr is distinct from old.orgnr
       or new.utenlandsk_id is distinct from old.utenlandsk_id
       or new.created_by is distinct from old.created_by
       or new.created_at is distinct from old.created_at then
        raise exception 'aksjonærens identitet er uforanderlig (aksjonær %)', old.id;
    end if;
    return new;
end;
$$;

create trigger shareholder_guard_trg
    before update or delete on shareholder
    for each row execute function shareholder_guard();

grant select, insert on shareholder to regnmed_app;
grant update (navn, adresse, postnummer, poststed, landkode, note)
    on shareholder to regnmed_app;

-- Hendelsene. Innsettings-bare, som bilag: en feilført hendelse rettes
-- med en motsatt hendelse, ikke ved å endre historien.
--
-- `antall` er alltid POSITIVT — retningen ligger i typen, slik at en rad
-- aldri kan motsi seg selv ved å påstå et negativt kjøp.
-- (regnmed-core::aksjebok::Transaksjonstype eier listen og retningen.)
create table share_event (
    id            uuid primary key,
    company_id    uuid not null references company (id),
    shareholder_id uuid not null references shareholder (id),
    type          text not null check (type <> ''),
    dato          date not null,
    antall        bigint not null check (antall > 0),
    -- Anskaffelsesverdi (tilgang) eller vederlag (avgang), når selskapet
    -- kjenner den. Oppgaven ber om den, men vet at den ikke alltid finnes.
    belop_ore     bigint,
    -- Motparten ved en overdragelse: to rader, én avgang og én tilgang,
    -- opprettet i samme transaksjon og pekende på hverandre.
    motpart_id    uuid references shareholder (id),
    note          text,
    -- Berører hendelsen hovedboken (innbetalt kapital, utbytte), peker
    -- den på bilaget.
    voucher_id    uuid references voucher (id),
    created_by    text not null check (created_by <> ''),
    created_at    timestamptz not null default now()
);

create index share_event_company_idx on share_event (company_id, dato, created_at);
create index share_event_holder_idx on share_event (shareholder_id, dato);

create function share_event_guard() returns trigger
language plpgsql as $$
begin
    raise exception 'aksjehendelser er innsettings-bare — rett med en motsatt hendelse';
end;
$$;

create trigger share_event_guard_trg
    before update or delete on share_event
    for each row execute function share_event_guard();

grant select, insert on share_event to regnmed_app;

-- Utbytte: ÉN rad per generalforsamlingsvedtak, ikke én per aksjonær.
--
-- Beløpet per aksjonær er antall aksjer på beslutningsdatoen ganger
-- utbytte per aksje — altså en funksjon over aksjeeierboken, ikke et
-- tall noen taster inn per eier. Da kan ikke summen av delene avvike
-- fra helheten, og oppgaven (post 21 selskapsnivå, post 21 aksjonærnivå)
-- stemmer per konstruksjon.
create table dividend (
    id             uuid primary key,
    company_id     uuid not null references company (id),
    -- Tidspunktet for generalforsamlingens vedtak (det oppgaven spør om).
    besluttet_dato date not null,
    per_aksje_ore  bigint not null check (per_aksje_ore > 0),
    note           text,
    voucher_id     uuid references voucher (id),
    created_by     text not null check (created_by <> ''),
    created_at     timestamptz not null default now()
);

create index dividend_company_idx on dividend (company_id, besluttet_dato);

create function dividend_guard() returns trigger
language plpgsql as $$
begin
    raise exception 'utbyttevedtak er innsettings-bare — et omgjort vedtak er et nytt vedtak';
end;
$$;

create trigger dividend_guard_trg
    before update or delete on dividend
    for each row execute function dividend_guard();

grant select, insert on dividend to regnmed_app;
