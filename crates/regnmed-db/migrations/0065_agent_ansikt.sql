-- An agent gets a face (docs/agent.md §8.1).
--
-- Initials come from `navn`, which already exists. These two columns are
-- the rest of it: a daisyUI semantic colour and an icon name from the
-- portal's own set. Deliberately NOT an uploaded image -- humans here
-- have initials too, because identity data belongs to regnid (#68), and
-- a face is not worth a user-content serving surface.
--
-- Both are plain, editable metadata: they are not hashed, not part of
-- any chain, and mean nothing to authorization. The CHECK only rejects
-- the empty string; the real allowlist lives next to the code that
-- turns a colour into a CSS class, because that is the one place that
-- can be wrong about it.
alter table integration add column avatar_farge text check (avatar_farge <> '');
alter table integration add column avatar_ikon text check (avatar_ikon <> '');

grant update (avatar_farge, avatar_ikon) on integration to regnmed_app;
