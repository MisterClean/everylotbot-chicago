import { DatabaseSync } from "node:sqlite";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export function createFixtureDatabase(): string {
  const path = join(mkdtempSync(join(tmpdir(), "everylot-test-")), "lots.db");
  const db = new DatabaseSync(path);
  db.exec(`
    CREATE TABLE lots (
      id TEXT PRIMARY KEY,
      address TEXT,
      lat REAL,
      lon REAL,
      posted_twitter TEXT DEFAULT '0',
      posted_bluesky TEXT DEFAULT '0'
    );
    INSERT INTO lots VALUES
      ('0428206041', '1 N EARLY ST, CHICAGO, IL 60601', 0, 0, '0', '0'),
      ('1431213017', '2 N SKIPPED ST, CHICAGO, IL 60601', 0, 0, '0', '0'),
      ('1431213018', '2023 N DAMEN AVE, CHICAGO, IL 60647', 0, 0, '0', 'https://bsky.app/profile/did:plc:test/post/old1'),
      ('1431213019', '2019 N DAMEN AVE, CHICAGO, IL 60647', 0, 0, '0', 'https://bsky.app/profile/did:plc:test/post/old2'),
      ('1431213020', '2017 N DAMEN AVE 1, CHICAGO, IL 60647', 0, 0, '0', 'https://bsky.app/profile/did:plc:test/post/old3'),
      ('1431213021', '2015 N DAMEN AVE, CHICAGO, IL 60647', 0, 0, '0', '0'),
      ('1431213024', '2038 N WINCHESTER AVE, CHICAGO, IL 60614', 0, 0, '0', '0');
  `);
  db.close();
  return path;
}
