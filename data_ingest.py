#!/usr/bin/env python3
import os
import sys
import requests
import csv
from io import StringIO
import sqlite3
import argparse
from dotenv import load_dotenv

def fetch_cook_county_rows(year='2023', city='CHICAGO', batch_size=50000):
    """
    Fetch raw rows from Cook County SODA (CSV) for the given tax year and city.
    Uses pagination to fetch all records, with a maximum of 50,000 records per page.
    """
    url = "https://datacatalog.cookcountyil.gov/resource/3723-97qp.csv"
    token = os.getenv("CHICAGO_DATA_PORTAL_TOKEN")
    if not token:
        raise ValueError("CHICAGO_DATA_PORTAL_TOKEN not found in environment")

    headers = {
        "X-App-Token": token
    }
    
    all_rows = []
    offset = 0
    
    while True:
        params = {
            "$query": f"""SELECT pin, pin10, year, prop_address_full,
                         prop_address_city_name, prop_address_state, prop_address_zipcode_1,
                         mail_address_name, mail_address_full, mail_address_city_name,
                         mail_address_state, mail_address_zipcode_1
                         WHERE (year IN ('{year}'))
                           AND caseless_one_of(prop_address_city_name, '{city}', '{city.title()}')
                         ORDER BY pin ASC
                         LIMIT {batch_size}
                         OFFSET {offset}"""
        }
        
        print(f"Fetching records {offset} to {offset + batch_size}...")
        r = requests.get(url, headers=headers, params=params)
        r.raise_for_status()

        # Parse CSV
        f = StringIO(r.text)
        reader = csv.DictReader(f)
        batch = list(reader)
        
        if not batch:  # No more records
            break
            
        all_rows.extend(batch)
        offset += batch_size
        
        print(f"Fetched {len(batch)} records in this batch")
        
        if len(batch) < batch_size:  # Last page
            break
    
    return all_rows

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

def create_local_db(records, db_path="cook_county_lots.db"):
    """
    Creates or overwrites the local SQLite DB with a 'lots' table
    unique by pin10. The 'id' column = pin10.
    If START_PIN10 is set in environment variables, marks all pins up to
    and including that PIN as '1' in posted_bluesky column.
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
        posted_twitter TEXT DEFAULT '0',
        posted_bluesky TEXT DEFAULT '0'
    )
    """)

    insert_sql = "INSERT INTO lots (id, address, lat, lon) VALUES (?, ?, ?, ?)"
    for row in records:
        pin10 = row.get('pin10', '')
        # build address string
        prop_address = row.get('prop_address_full', '').strip()
        prop_city = row.get('prop_address_city_name', '').strip()
        prop_state = row.get('prop_address_state', '').strip()
        prop_zip = row.get('prop_address_zipcode_1', '').strip()

        # final concatenated address: "123 Main St, Chicago, IL 60601"
        final_address = f"{prop_address}, {prop_city}, {prop_state} {prop_zip}".strip(", ")

        c.execute(insert_sql, (pin10, final_address, 0.0, 0.0))

    # If START_PIN10 is set, mark all pins up to and including it as posted
    start_pin = os.getenv('START_PIN10')
    if start_pin:
        print(f"\nMarking all pins up to and including {start_pin} as posted...")
        c.execute("""
            UPDATE lots 
            SET posted_bluesky = '1' 
            WHERE CAST(id AS INTEGER) <= CAST(? AS INTEGER)
        """, (start_pin,))
        rows_marked = c.execute("SELECT COUNT(*) FROM lots WHERE posted_bluesky = '1'").fetchone()[0]
        print(f"Marked {rows_marked:,d} pins as posted")

    conn.commit()
    conn.close()

def main():
    load_dotenv()

    parser = argparse.ArgumentParser(description='Fetch and process Cook County property data')
    parser.add_argument('--year', type=str, default='2023', help='Tax year to fetch')
    parser.add_argument('--city', type=str, default='CHICAGO', help='City to filter by')
    parser.add_argument('--db', type=str, default='cook_county_lots.db', help='Output database path')
    args = parser.parse_args()

    try:
        print(f"Fetching Cook County data for {args.city} ({args.year})...")
        print("This may take a while as we paginate through all records...")
        rows = fetch_cook_county_rows(args.year, args.city)
        print(f"\nFetched {len(rows):,d} total records")

        print("\nTransforming to unique PIN10 records...")
        unique_records = transform_rows_to_unique_pin10(rows)
        print(f"Reduced to {len(unique_records):,d} unique PIN10 records")

        print(f"\nCreating local database at {args.db}...")
        create_local_db(unique_records, args.db)
        print("Database created successfully!")

    except Exception as e:
        print(f"Error: {str(e)}", file=sys.stderr)
        sys.exit(1)

if __name__ == '__main__':
    main()
