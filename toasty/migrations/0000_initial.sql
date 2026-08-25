CREATE TABLE "characters" (
    "id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    "account_id" INTEGER NOT NULL,
    "gender" TEXT NOT NULL CHECK ("gender" IN ('male', 'female')),
    "savedata" BLOB,
    "name" TEXT NOT NULL,
    "description" TEXT NOT NULL,
    "gr" INTEGER NOT NULL,
    "hr" INTEGER NOT NULL,
    "weapon_type" TEXT NOT NULL CHECK ("weapon_type" IN ('sword_and_shield', 'heavy_bowgun', 'hammer', 'great_sword', 'lance', 'light_bowgun', 'long_sword', 'dual_blades', 'hunting_horn', 'gunlance', 'bow', 'tonfa', 'switch_axe', 'magnet_spike')),
    "deleted_at" TEXT
);
-- #[toasty::breakpoint]
CREATE INDEX "index_characters_by_account_id" ON "characters" ("account_id");
-- #[toasty::breakpoint]
CREATE TABLE "sign_sessions" (
    "id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    "account_id" INTEGER NOT NULL,
    "token_hash" BLOB NOT NULL,
    "validity_starts_at" TEXT NOT NULL,
    "validity_expires_at" TEXT NOT NULL
);
-- #[toasty::breakpoint]
CREATE INDEX "index_sign_sessions_by_account_id" ON "sign_sessions" ("account_id");
-- #[toasty::breakpoint]
CREATE TABLE "character_sign_in_records" (
    "id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    "character_id" INTEGER NOT NULL,
    "signed_in_at" TEXT NOT NULL
);
-- #[toasty::breakpoint]
CREATE INDEX "index_character_sign_in_records_by_character_id_and_signed_in_at_and_id" ON "character_sign_in_records" ("character_id", "signed_in_at", "id");
-- #[toasty::breakpoint]
CREATE TABLE "account_sign_in_records" (
    "id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    "account_id" INTEGER NOT NULL,
    "signed_in_at" TEXT NOT NULL
);
-- #[toasty::breakpoint]
CREATE INDEX "index_account_sign_in_records_by_account_id_and_signed_in_at_and_id" ON "account_sign_in_records" ("account_id", "signed_in_at", "id");
-- #[toasty::breakpoint]
CREATE TABLE "mezeporta_festa_stalls" (
    "festa_id" INTEGER NOT NULL,
    "position" INTEGER NOT NULL,
    "stall" TEXT NOT NULL CHECK ("stall" IN ('tokotoko_partnya', 'unknown3', 'volpakkun_together', 'unknown5', 'unknown6', 'unknown7', 'unknown8', 'unknown9', 'unknown10')),
    PRIMARY KEY ("festa_id", "position")
);
-- #[toasty::breakpoint]
CREATE UNIQUE INDEX "index_mezeporta_festa_stalls_by_festa_id_and_stall" ON "mezeporta_festa_stalls" ("festa_id", "stall");
-- #[toasty::breakpoint]
CREATE TABLE "account_return_periods" (
    "id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    "account_id" INTEGER NOT NULL,
    "starts_at" TEXT NOT NULL,
    "expires_at" TEXT NOT NULL
);
-- #[toasty::breakpoint]
CREATE INDEX "index_account_return_periods_by_account_id_and_id" ON "account_return_periods" ("account_id", "id");
-- #[toasty::breakpoint]
CREATE TABLE "mezeporta_festas" (
    "id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    "starts_at" TEXT NOT NULL,
    "expires_at" TEXT NOT NULL,
    "solo_ticket_allowance" INTEGER NOT NULL,
    "group_ticket_allowance" INTEGER NOT NULL
);
-- #[toasty::breakpoint]
CREATE TABLE "accounts" (
    "id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    "username" TEXT NOT NULL,
    "password_hash" TEXT NOT NULL,
    "rights" INTEGER NOT NULL
);
-- #[toasty::breakpoint]
CREATE UNIQUE INDEX "index_accounts_by_username" ON "accounts" ("username");
