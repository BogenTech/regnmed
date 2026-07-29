-- The invitation e-mail actually goes out (#66, docs/auth.md §7).
--
-- Until now `POST /companies/{id}/invitations` created the invitation and
-- answered honestly `"epost_sendt": false` — nothing was ever sent. The
-- honesty was right; the gap was real. An invitation nobody receives is a
-- feature that does not exist.
--
-- The send rides the SAME rail and the SAME log as every other outbound
-- mail (#32): one `utsendelse` row per send, its id doubling as the
-- queue's Nats-Msg-Id. So this migration only widens what an utsendelse
-- row may point at.
--
-- **No secret token travels in the mail.** The link goes to the portal's
-- front page, nothing more. Redemption stays what it was: the address
-- logs in through the IdP and `/me` matches it. A forwarded invitation
-- e-mail therefore grants the forwarder nothing — which is exactly why
-- there is nothing in it worth stealing.

alter table utsendelse
    add column invitation_id uuid references company_invitation (id);

-- The row must still point at SOMETHING. Widened, not dropped: an
-- utsendelse that refers to nothing is a log line that cannot be audited.
alter table utsendelse drop constraint utsendelse_check;
alter table utsendelse add constraint utsendelse_check
    check (invoice_id is not null
        or reminder_id is not null
        or invitation_id is not null);

create index utsendelse_invitation on utsendelse (invitation_id);
