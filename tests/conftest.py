import pytest
import sqlite3
from io import BytesIO
import os

@pytest.fixture
def sample_image():
    """Create a sample image for testing"""
    return BytesIO(b"fake-image-data")

@pytest.fixture
def base_test_db():
    """Create a base test database schema"""
    def _create_db(path):
        conn = sqlite3.connect(path)
        c = conn.cursor()
        
        c.execute("""
            CREATE TABLE lots (
                id TEXT PRIMARY KEY,
                address TEXT,
                lat REAL,
                lon REAL,
                tweeted TEXT DEFAULT '0',
                floors INTEGER
            )
        """)
        
        conn.commit()
        conn.close()
    
    return _create_db

@pytest.fixture
def sample_lot_data():
    """Sample property data for testing"""
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
        }
    ]

@pytest.fixture
def mock_base_env(monkeypatch):
    """Set up base environment variables needed for testing"""
    env_vars = {
        "GOOGLE_API_KEY": "test_google_key",
        "CHICAGO_DATA_PORTAL_TOKEN": "test_data_token",
        "SEARCH_FORMAT": "{address}, Chicago, IL",
        "PRINT_FORMAT": "{address}",
        "DATABASE_PATH": "test.db"
    }
    for key, value in env_vars.items():
        monkeypatch.setenv(key, value)
    return env_vars

@pytest.fixture
def mock_twitter_env(monkeypatch):
    """Set up Twitter-specific environment variables"""
    env_vars = {
        "TWITTER_CONSUMER_KEY": "test_consumer_key",
        "TWITTER_CONSUMER_SECRET": "test_consumer_secret",
        "TWITTER_ACCESS_TOKEN": "test_access_token",
        "TWITTER_ACCESS_TOKEN_SECRET": "test_access_token_secret",
        "ENABLE_TWITTER": "true"
    }
    for key, value in env_vars.items():
        monkeypatch.setenv(key, value)
    return env_vars

@pytest.fixture
def mock_bluesky_env(monkeypatch):
    """Set up Bluesky-specific environment variables"""
    env_vars = {
        "BLUESKY_IDENTIFIER": "test.bsky.social",
        "BLUESKY_PASSWORD": "test_password",
        "ENABLE_BLUESKY": "true"
    }
    for key, value in env_vars.items():
        monkeypatch.setenv(key, value)
    return env_vars

@pytest.fixture
def test_db_with_data(tmp_path, base_test_db):
    """Create a test database with sample data"""
    db_path = str(tmp_path / "test.db")
    base_test_db(db_path)
    
    conn = sqlite3.connect(db_path)
    c = conn.cursor()
    
    test_data = [
        ('1407115016', '123 Main St', 41.8781, -87.6298, '0', 2),
        ('1407115017', '125 Main St', 41.8782, -87.6299, '0', 3),
        ('1407115018', '127 Main St', 41.8783, -87.6300, '1', 4),
    ]
    c.executemany(
        "INSERT INTO lots (id, address, lat, lon, tweeted, floors) VALUES (?, ?, ?, ?, ?, ?)",
        test_data
    )
    
    conn.commit()
    conn.close()
    return db_path

@pytest.fixture
def clean_test_db(tmp_path, base_test_db):
    """Create an empty test database with schema"""
    db_path = str(tmp_path / "test.db")
    base_test_db(db_path)
    return db_path

@pytest.fixture
def mock_responses():
    """Configure common mock responses for testing"""
    return {
        'streetview': b"fake-image-data",
        'geocode': {
            'results': [{
                'geometry': {
                    'location': {
                        'lat': 41.8781,
                        'lng': -87.6298
                    }
                }
            }]
        }
    }

@pytest.fixture
def setup_logging():
    """Configure logging for tests"""
    import logging
    logging.basicConfig(
        level=logging.DEBUG,
        format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
    )
