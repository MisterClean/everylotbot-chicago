import pytest
import sqlite3
import os
from unittest.mock import patch, mock_open
import responses
from io import StringIO
import json
from data_ingest import (
    fetch_cook_county_rows,
    transform_rows_to_unique_pin10,
    create_local_db
)

# Sample test data
SAMPLE_CSV_DATA = '''pin,pin10,year,prop_address_full,prop_address_city_name,prop_address_state,prop_address_zipcode_1
14071150160000,1407115016,2023,123 MAIN ST,CHICAGO,IL,60601
14071150170000,1407115017,2023,125 MAIN ST,CHICAGO,IL,60601
14071150180000,1407115017,2023,127 MAIN ST,CHICAGO,IL,60601'''

@pytest.fixture
def sample_rows():
    """Sample property data rows"""
    return [
        {
            'pin': '14071150160000',
            'pin10': '1407115016',
            'year': '2023',
            'prop_address_full': '123 MAIN ST',
            'prop_address_city_name': 'CHICAGO',
            'prop_address_state': 'IL',
            'prop_address_zipcode_1': '60601'
        },
        {
            'pin': '14071150170000',
            'pin10': '1407115017',
            'year': '2023',
            'prop_address_full': '125 MAIN ST',
            'prop_address_city_name': 'CHICAGO',
            'prop_address_state': 'IL',
            'prop_address_zipcode_1': '60601'
        },
        {
            'pin': '14071150180000',
            'pin10': '1407115017',
            'year': '2023',
            'prop_address_full': '127 MAIN ST',
            'prop_address_city_name': 'CHICAGO',
            'prop_address_state': 'IL',
            'prop_address_zipcode_1': '60601'
        }
    ]

@pytest.fixture
def test_db_path(tmp_path):
    """Create a temporary database path"""
    return str(tmp_path / "test_lots.db")

class TestDataIngest:
    @responses.activate
    def test_fetch_cook_county_rows(self, monkeypatch):
        """Test fetching rows from Cook County API"""
        # Mock environment variable
        monkeypatch.setenv("CHICAGO_DATA_PORTAL_TOKEN", "test_token")
        
        # Mock the API response
        responses.add(
            responses.GET,
            "https://datacatalog.cookcountyil.gov/resource/3723-97qp.csv",
            body=SAMPLE_CSV_DATA,
            status=200,
            content_type="text/csv"
        )
        
        rows = fetch_cook_county_rows(year="2023", city="CHICAGO")
        
        assert len(rows) == 3
        assert rows[0]['pin'] == '14071150160000'
        assert rows[0]['pin10'] == '1407115016'
        assert rows[0]['prop_address_full'] == '123 MAIN ST'

    def test_transform_rows_to_unique_pin10(self, sample_rows):
        """Test deduplication by PIN10"""
        unique_records = transform_rows_to_unique_pin10(sample_rows)
        
        # Should have 2 unique PIN10s
        assert len(unique_records) == 2
        
        # First record should be kept for duplicate PIN10
        pin10_records = [r for r in unique_records if r['pin10'] == '1407115017']
        assert len(pin10_records) == 1
        assert pin10_records[0]['prop_address_full'] == '125 MAIN ST'

    def test_create_local_db(self, sample_rows, test_db_path):
        """Test database creation with sample records"""
        create_local_db(sample_rows, test_db_path)
        
        # Verify database contents
        conn = sqlite3.connect(test_db_path)
        cursor = conn.cursor()
        
        # Check table structure
        cursor.execute("PRAGMA table_info(lots)")
        columns = [row[1] for row in cursor.fetchall()]
        expected_columns = ['id', 'address', 'lat', 'lon', 'tweeted']
        assert all(col in columns for col in expected_columns)
        
        # Check record count
        cursor.execute("SELECT COUNT(*) FROM lots")
        count = cursor.fetchone()[0]
        assert count == len(sample_rows)
        
        # Check first record
        cursor.execute("SELECT * FROM lots WHERE id=?", (sample_rows[0]['pin10'],))
        record = cursor.fetchone()
        expected_address = "123 MAIN ST, CHICAGO, IL 60601"
        assert record[1] == expected_address  # address field
        
        conn.close()

    def test_fetch_cook_county_rows_no_token(self, monkeypatch):
        """Test error handling when token is missing"""
        monkeypatch.delenv("CHICAGO_DATA_PORTAL_TOKEN", raising=False)
        
        with pytest.raises(ValueError, match="CHICAGO_DATA_PORTAL_TOKEN not found"):
            fetch_cook_county_rows()

    @responses.activate
    def test_fetch_cook_county_rows_api_error(self, monkeypatch):
        """Test handling of API errors"""
        monkeypatch.setenv("CHICAGO_DATA_PORTAL_TOKEN", "test_token")
        
        responses.add(
            responses.GET,
            "https://datacatalog.cookcountyil.gov/resource/3723-97qp.csv",
            status=500
        )
        
        with pytest.raises(requests.exceptions.HTTPError):
            fetch_cook_county_rows()

    def test_create_local_db_invalid_path(self, sample_rows, tmp_path):
        """Test database creation with invalid path"""
        invalid_path = str(tmp_path / "nonexistent" / "test.db")
        
        with pytest.raises(sqlite3.OperationalError):
            create_local_db(sample_rows, invalid_path)
