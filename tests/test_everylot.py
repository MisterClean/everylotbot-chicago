import pytest
import sqlite3
import responses
from unittest.mock import Mock, patch
from io import BytesIO
import os
from everylot.everylot import EveryLot

@pytest.fixture
def test_db_path(tmp_path):
    """Create a temporary test database"""
    db_path = str(tmp_path / "test.db")
    conn = sqlite3.connect(db_path)
    c = conn.cursor()
    
    # Create test table
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
    
    # Insert test data
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
def mock_env(monkeypatch):
    """Set up mock environment variables"""
    monkeypatch.setenv("SEARCH_FORMAT", "{address}, Chicago, IL")
    monkeypatch.setenv("PRINT_FORMAT", "{address}")

class TestEveryLot:
    def test_initialization_next_untweeted(self, test_db_path):
        """Test initialization finding next untweeted lot"""
        el = EveryLot(test_db_path)
        
        assert el.lot is not None
        assert el.lot['id'] == '1407115016'
        assert el.lot['address'] == '123 Main St'
        assert el.lot['tweeted'] == '0'

    def test_initialization_specific_id(self, test_db_path):
        """Test initialization with specific ID"""
        el = EveryLot(test_db_path, id_='1407115017')
        
        assert el.lot is not None
        assert el.lot['id'] == '1407115017'
        assert el.lot['address'] == '125 Main St'

    def test_initialization_no_results(self, test_db_path):
        """Test initialization when no matching lots found"""
        el = EveryLot(test_db_path, id_='nonexistent')
        assert el.lot is None

    def test_aim_camera(self, test_db_path):
        """Test camera angle calculations based on building height"""
        el = EveryLot(test_db_path)
        
        # Test different floor counts
        test_cases = [
            (2, (65, 10)),  # Default
            (3, (72, 10)),
            (4, (76, 15)),
            (5, (81, 20)),
            (6, (86, 20)),
            (8, (90, 25)),
            (10, (90, 30))
        ]
        
        for floors, expected in test_cases:
            el.lot['floors'] = floors
            fov, pitch = el.aim_camera()
            assert (fov, pitch) == expected

    @responses.activate
    def test_get_streetview_image(self, test_db_path):
        """Test fetching Street View image"""
        # Mock Street View API response
        responses.add(
            responses.GET,
            "https://maps.googleapis.com/maps/api/streetview",
            body=b"fake-image-data",
            status=200,
            content_type="image/jpeg"
        )
        
        el = EveryLot(test_db_path)
        image = el.get_streetview_image("test_key")
        
        assert isinstance(image, BytesIO)
        image_data = image.getvalue()
        assert image_data == b"fake-image-data"

    @responses.activate
    def test_streetviewable_location_with_geocoding(self, test_db_path):
        """Test location determination with geocoding"""
        # Mock Geocoding API response
        responses.add(
            responses.GET,
            "https://maps.googleapis.com/maps/api/geocode/json",
            json={
                "results": [{
                    "geometry": {
                        "location": {
                            "lat": 41.8781,
                            "lng": -87.6298
                        }
                    }
                }]
            },
            status=200
        )
        
        el = EveryLot(test_db_path)
        location = el.streetviewable_location("test_key")
        
        # Should use formatted address since geocoded location is within bounds
        assert location == "123 Main St, Chicago, IL"

    @responses.activate
    def test_streetviewable_location_geocoding_too_far(self, test_db_path):
        """Test fallback to coordinates when geocoded location is too far"""
        # Mock Geocoding API response with location outside bounds
        responses.add(
            responses.GET,
            "https://maps.googleapis.com/maps/api/geocode/json",
            json={
                "results": [{
                    "geometry": {
                        "location": {
                            "lat": 42.0,  # Too far from test coordinates
                            "lng": -88.0
                        }
                    }
                }]
            },
            status=200
        )
        
        el = EveryLot(test_db_path)
        location = el.streetviewable_location("test_key")
        
        # Should fall back to coordinates
        assert location == "41.8781,-87.6298"

    def test_compose(self, test_db_path):
        """Test composing post data"""
        el = EveryLot(test_db_path)
        post_data = el.compose("test_media_id")
        
        assert post_data["status"] == "123 Main St"
        assert post_data["lat"] == 41.8781
        assert post_data["long"] == -87.6298
        assert post_data["media_ids"] == ["test_media_id"]

    def test_mark_as_tweeted(self, test_db_path):
        """Test marking a lot as tweeted"""
        el = EveryLot(test_db_path)
        original_id = el.lot['id']
        
        el.mark_as_tweeted("test_post_id")
        
        # Verify database update
        conn = sqlite3.connect(test_db_path)
        c = conn.cursor()
        c.execute("SELECT tweeted FROM lots WHERE id = ?", (original_id,))
        tweeted_value = c.fetchone()[0]
        conn.close()
        
        assert tweeted_value == "test_post_id"

    def test_custom_format_strings(self, test_db_path):
        """Test custom search and print format strings"""
        el = EveryLot(
            test_db_path,
            search_format="{address}, Custom City",
            print_format="Location: {address}"
        )
        
        # Test search format
        location = el.streetviewable_location("test_key")
        assert location == "123 Main St, Custom City"
        
        # Test print format
        post_data = el.compose()
        assert post_data["status"] == "Location: 123 Main St"
