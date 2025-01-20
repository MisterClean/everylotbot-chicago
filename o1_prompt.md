Below is an updated, consolidated set of requirements and an implementation plan for your engineer (“Claude”). These instructions incorporate:

- The new **bots-example.yaml** file and how it was previously used.  
- The additional data transformation logic required for **pin14** vs. **pin10**.  
- A new mechanism to specify which property (PIN10) to start posting from.  

## 1. Overview of Changes

1. **Configuration / Secrets**  
   - We now have two potential configuration files:  
     - A legacy `bots.yaml` (or `bots-example.yaml`), used in the older code for storing Twitter credentials and a Street View key.  
     - A new `.env` approach (using `python-dotenv`) storing tokens (Data Portal token, Bluesky credentials, etc.).  
   - We can either merge them into a single `.env` for simplicity or maintain partial compatibility with the old `bots.yaml`.  

2. **Data Module**  
   - Fetch data from Cook County SODA endpoint for a specific tax year (default 2023) and for city = “CHICAGO.”  
   - Sort records by **pin14** ascending (the field is `pin` in the CSV).  
   - De-duplicate by **pin10** (the field is `pin10`). If multiple rows share the same pin10, keep only the first record in ascending pin14 order.  
   - Build a final list where each row is unique by pin10.  
   - Construct the final “address” field by concatenating `prop_address_full, prop_address_city_name, prop_address_state, prop_address_zipcode_1`.  
   - Store these in a local SQLite database with columns like:  
     - `id` (set this to `pin10`)  
     - `address` (the concatenated property address)  
     - `tweeted` (default 0)  
     - `lat`, `lon` (optionally 0 or null if you are not bulk geocoding)  

3. **Posting Sequence**  
   - The bot should iterate over rows in ascending order by `id` (which is the pin10).  
   - **New Setting**: Let the user specify (in `setup.py` or `.env`) the **starting pin10**. If that is set, skip ahead to that row in the DB.  

4. **Bluesky Posting & Twitter Posting**  
   - Provide modules for Bluesky (using `atproto`) and Twitter (using updated `tweepy` or direct API calls).  
   - Provide a toggle in `setup.py` or `.env` for `ENABLE_TWITTER` / `ENABLE_BLUESKY`.  
   - If both are enabled, post to both; if only Bluesky is enabled, post there alone, etc.  

5. **Modernization to Python 3.10+**  
   - Drop Python 2 compatibility, clean up legacy code.  
   - Provide a `requirements.txt`.  

---

## 2. Detailed Instructions

### 2.1 Configuration & Secrets

1. **bots-example.yaml**  
   ```yaml
   ---
   streetview: abc123...  # Old Street View key
   apps:
       example_app_name:
           consumer_secret: def456...
           consumer_key: ghi789...
   users:
       example_screen_name:
           key: jkl123...
           secret: mno456...
           app: example_app_name
   ```
   - Historically used for Twitter credentials.  
   - Also includes a single line for the Street View API key (`streetview`).  

2. **.env**  
   - We suggest placing all new tokens here (Data Portal token, Bluesky creds, optional new Twitter keys). For example:
     ```bash
     # Cook County data token
     CHICAGO_DATA_PORTAL_TOKEN=xxxxxx

     # Google Street View & Geocoding
     GOOGLE_API_KEY=abc123...

     # Bluesky
     BLUESKY_IDENTIFIER=me.bsky.social
     BLUESKY_PASSWORD=supersecret

     # Twitter (if using)
     TWITTER_CONSUMER_KEY=ghi789...
     TWITTER_CONSUMER_SECRET=def456...
     TWITTER_ACCESS_TOKEN=jkl123...
     TWITTER_ACCESS_TOKEN_SECRET=mno456...

     # Toggles
     ENABLE_TWITTER=false
     ENABLE_BLUESKY=true

     # Starting PIN10
     START_PIN10=1234567890
     ```
   - Then load it in Python via `python-dotenv` or similar.  

3. **Merging Approaches**  
   - Option A: Migrate the old YAML config into `.env` (and remove `bots.yaml` entirely).  
   - Option B: Keep partial compatibility for older code that uses `bots.yaml`, but anything new uses `.env`.  

---

### 2.2 Data Ingestion & Transformation

Create a new module, e.g., `data_ingest.py`:

1. **Fetch & Parse**  
   ```python
   import os
   import requests
   import csv
   from io import StringIO
   import sqlite3

   def fetch_cook_county_rows(year='2023', city='CHICAGO'):
       """
       Fetch raw rows from Cook County SODA (CSV) for the given tax year and city.
       """
       url = "https://datacatalog.cookcountyil.gov/resource/3723-97qp.csv"
       headers = {
           "X-App-Token": os.getenv("CHICAGO_DATA_PORTAL_TOKEN", "")
       }
       params = {
           "$query": f"""SELECT pin, pin10, year, prop_address_full,
                         prop_address_city_name, prop_address_state, prop_address_zipcode_1,
                         mail_address_name, mail_address_full, mail_address_city_name,
                         mail_address_state, mail_address_zipcode_1
                         WHERE (year IN ('{year}'))
                           AND caseless_one_of(prop_address_city_name, '{city}', '{city.title()}')
                         ORDER BY pin ASC"""
       }
       r = requests.get(url, headers=headers, params=params)
       r.raise_for_status()

       # Parse CSV
       f = StringIO(r.text)
       reader = csv.DictReader(f)
       rows = list(reader)
       return rows
   ```

2. **Transform**  
   - We need to group by `pin10`, keeping only the first record in ascending order of `pin` (pin14).
   ```python
   def transform_rows_to_unique_pin10(rows):
       """
       Sorts rows by pin14 ascending (already done in the SODA query),
       then de-duplicates on pin10, keeping the first occurrence.
       Returns a list of unique records by pin10.
       """
       seen_pin10 = set()
       unique_records = []
       for row in rows:
           pin10 = row['pin10']
           if pin10 not in seen_pin10:
               seen_pin10.add(pin10)
               unique_records.append(row)
       return unique_records
   ```

3. **Create SQLite**  
   - The final DB schema should have columns:
     - `id` (TEXT) — store the pin10 here  
     - `address` (TEXT) — `"prop_address_full, prop_address_city_name, prop_address_state prop_address_zipcode_1"`  
     - `tweeted` (TEXT default ‘0’)  
     - `lat` (REAL, optional)  
     - `lon` (REAL, optional)  
   ```python
   def create_local_db(records, db_path="cook_county_lots.db"):
       """
       Creates or overwrites the local SQLite DB with a 'lots' table
       unique by pin10. The 'id' column = pin10.
       """
       conn = sqlite3.connect(db_path)
       c = conn.cursor()

       c.execute("DROP TABLE IF EXISTS lots;")
       c.execute("""
       CREATE TABLE lots (
         id TEXT PRIMARY KEY,
         address TEXT,
         lat REAL,
         lon REAL,
         tweeted TEXT
       )
       """)

       insert_sql = "INSERT INTO lots (id, address, lat, lon, tweeted) VALUES (?, ?, ?, ?, ?)"
       for row in records:
           pin10 = row.get('pin10', '')
           # build address string
           prop_address = row.get('prop_address_full', '').strip()
           prop_city = row.get('prop_address_city_name', '').strip()
           prop_state = row.get('prop_address_state', '').strip()
           prop_zip = row.get('prop_address_zipcode_1', '').strip()

           # final concatenated address: "123 Main St, Chicago, IL 60601"
           final_address = f"{prop_address}, {prop_city}, {prop_state} {prop_zip}".strip(", ")

           c.execute(insert_sql, (pin10, final_address, 0.0, 0.0, '0'))

       conn.commit()
       conn.close()
   ```

4. **Usage**:  
   ```bash
   # Example usage:
   python data_ingest.py --year 2023 --city CHICAGO --db cook_county_lots.db
   ```
   - Where that script calls `fetch_cook_county_rows()`, then `transform_rows_to_unique_pin10()`, then `create_local_db()`.  

---

### 2.3 Main Bot Flow

The existing code in `boy.py` (or `everylot/bot.py`) will be updated to:

1. Read in environment toggles (`ENABLE_TWITTER`, `ENABLE_BLUESKY`).  
2. Connect to the newly created SQLite DB (e.g. `cook_county_lots.db`).  
3. On each run, pick the **next** record (by ascending `id` = pin10) that has `tweeted=0`.  
   - Or if `START_PIN10` is set, skip until we find that record.  
4. Generate the Street View image (using `GOOGLE_API_KEY` or fallback from `bots.yaml` if needed).  
5. Compose the text from the `address` field.  
6. Post to Bluesky (if enabled) and/or Twitter (if enabled).  
7. Mark that row as `tweeted=<postId>` in the DB.  

#### Start on a Specific PIN10

In `setup.py` or `.env`:

```bash
START_PIN10=1234567890
```

Then in your `boy.py` logic:

```python
start_pin10 = os.getenv("START_PIN10")
...
# After connecting to the DB
if start_pin10:
    # Query: SELECT * FROM lots WHERE id >= start_pin10 AND tweeted='0' ORDER BY id LIMIT 1
    # or do a more robust approach if pin10 is not purely numeric
```

If we find that record, tweet it; otherwise proceed to the next.

---

### 2.4 Posting to Bluesky & Twitter

**Bluesky** (in `bluesky_module.py`):
```python
from datetime import datetime
from atproto import Client
import os

def post_to_bluesky(status_text, image_path=None):
    client = Client()
    identifier = os.getenv("BLUESKY_IDENTIFIER")
    password = os.getenv("BLUESKY_PASSWORD")
    client.login(identifier, password)

    record = {
        "collection": "app.bsky.feed.post",
        "repo": identifier,
        "record": {
            "text": status_text,
            "createdAt": datetime.utcnow().isoformat() + "Z",
        }
    }

    if image_path:
        with open(image_path, "rb") as f:
            upload_resp = client.com.atproto.repo.upload_blob(f.read(), "image/jpeg")
        record["record"]["embed"] = {
            "$type": "app.bsky.embed.images",
            "images": [{
                "image": upload_resp["blob"],
                "alt": "Property photo"
            }]
        }

    resp = client.com.atproto.repo.create_record(**record)
    return resp  # Contains the URI, etc.
```

**Twitter** (in `twitter_module.py`):
```python
import tweepy
import os

def post_to_twitter(status_text, image_path=None):
    consumer_key = os.getenv("TWITTER_CONSUMER_KEY")
    consumer_secret = os.getenv("TWITTER_CONSUMER_SECRET")
    access_token = os.getenv("TWITTER_ACCESS_TOKEN")
    access_token_secret = os.getenv("TWITTER_ACCESS_TOKEN_SECRET")

    auth = tweepy.OAuth1UserHandler(consumer_key, consumer_secret, access_token, access_token_secret)
    api = tweepy.API(auth)

    if image_path:
        media = api.media_upload(image_path)
        tweet = api.update_status(status=status_text, media_ids=[media.media_id_string])
    else:
        tweet = api.update_status(status=status_text)

    return tweet.id
```

In `boy.py`, after you get the status text & image:
```python
enable_twitter = os.getenv("ENABLE_TWITTER", "false").lower() == "true"
enable_bluesky = os.getenv("ENABLE_BLUESKY", "true").lower() == "true"

post_ids = []
if enable_bluesky:
    resp = post_to_bluesky(status_text, temp_img_path)
    post_ids.append(resp["uri"] if resp else "bluesky_error")

if enable_twitter:
    tweet_id = post_to_twitter(status_text, temp_img_path)
    post_ids.append(str(tweet_id))

# Mark as tweeted:
db.execute("UPDATE lots SET tweeted=? WHERE id=?", (",".join(post_ids), current_pin10,))
```

---

### 2.5 Modernize to Python 3.10

- Remove Python 2 imports like `from __future__ import unicode_literals`.  
- Confirm print statements, f-strings, etc.  
- Create `requirements.txt`:
  ```bash
  requests>=2.28
  python-dotenv>=0.21
  atproto>=0.2
  tweepy>=4.0
  ```
- Validate it runs on Python 3.10.  

---

## 3. Step-by-Step Hand-off Summary

1. **Add `.env`**  
   - Place your credentials (data portal token, Google key, Bluesky creds, optional Twitter) in `.env`.  
   - If desired, keep or migrate from `bots.yaml` so everything is in `.env`.  

2. **Implement `data_ingest.py`**  
   - **fetch_cook_county_rows(year, city)**: fetch & parse.  
   - **transform_rows_to_unique_pin10(rows)**: deduplicate by pin10, sorted by pin14.  
   - **create_local_db(records, db_path)**: write to `lots` table.  
   - Optionally create a CLI entry point (e.g., `python data_ingest.py --year 2023 --city CHICAGO`).  

3. **Refactor the Bot Logic (`boy.py`)**  
   - Load toggles from `.env` (`ENABLE_TWITTER`, `ENABLE_BLUESKY`, `START_PIN10`, etc.).  
   - Connect to `cook_county_lots.db`.  
   - If `START_PIN10` is set, find that row or the next row in ascending order.  
   - Instantiate `EveryLot` or a modified version that picks the next property with `tweeted=0`.  
   - Use the existing Street View logic (with `GOOGLE_API_KEY`).  
   - Post to Bluesky / Twitter as configured.  
   - Mark row as tweeted with the post or tweet ID.  

4. **Create `bluesky_module.py` and `twitter_module.py`**  
   - Each has a simple `post_to_*` function.  

5. **Test**  
   - Validate that it runs in Python 3.10.  
   - Confirm deduplication of pin10 and the final address format.  

6. **Deploy**  
   - Set up a cron to run `boy.py` (or the new consolidated script) on a desired schedule.  

---

## 4. Final Notes

- **Sequential by PIN10**: You’ll be pulling from the sorted DB. Each run picks the next unposted record.  
- **Starting Property**: The `START_PIN10` environment variable or a command-line argument ensures we skip forward if we want.  
- **bots-example.yaml** can be used as a reference or for backward compatibility. In the modern approach, we prefer `.env`.  
- **Database Growth**: If Cook County is large, you may want to fetch and generate the DB once, then run the bot daily from that local DB.  

With these instructions, “Claude” should have all the details needed to implement the final solution. Good luck!