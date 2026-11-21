-- Skatteetaten stops creating Dialogporten dialogs for skattekort orders on
-- 2026-12-15, so an order receipt may no longer carry a dialog reference.
-- regnmed polls with the order reference and never needed it.
alter table tax_card_order alter column dialog_reference drop not null;
