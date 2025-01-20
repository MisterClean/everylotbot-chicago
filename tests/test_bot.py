import pytest
import os
from unittest.mock import Mock, patch, ANY
from io import BytesIO
import sqlite3
import logging
from everylot.bot import main

@pytest.fixture
def test_db_path(tmp_path):
    """Create a temporary test database"""
    db_path = str(tmp_path / "test.db")
    conn = sqlite3.connect(db_path)
    c = conn.cursor()
    
    c.execute("""
        CREATE TABLE lots (
            id TEXT PRIMARY KEY,
            address TEXT,
            lat REAL,
            lon REAL,
            posted_twitter TEXT DEFAULT '0',
            posted_bluesky TEXT DEFAULT '0',
            floors INTEGER
        )
    """)
    
    test_data = [
        ('1407115016', '123 Main St', 41.8781, -87.6298, '0', '0', 2),
        ('1407115017', '125 Main St', 41.8782, -87.6299, '0', '0', 3),
    ]
    c.executemany(
        "INSERT INTO lots (id, address, lat, lon, posted_twitter, posted_bluesky, floors) VALUES (?, ?, ?, ?, ?, ?, ?)",
        test_data
    )
    
    conn.commit()
    conn.close()
    return db_path

@pytest.fixture
def mock_env(monkeypatch):
    """Set up mock environment variables"""
    env_vars = {
        "GOOGLE_API_KEY": "test_google_key",
        "BLUESKY_IDENTIFIER": "test.bsky.social",
        "BLUESKY_PASSWORD": "test_password",
        "TWITTER_CONSUMER_KEY": "test_consumer_key",
        "TWITTER_CONSUMER_SECRET": "test_consumer_secret",
        "TWITTER_ACCESS_TOKEN": "test_access_token",
        "TWITTER_ACCESS_TOKEN_SECRET": "test_access_token_secret",
        "ENABLE_BLUESKY": "true",
        "ENABLE_TWITTER": "true",
        "SEARCH_FORMAT": "{address}, Chicago, IL",
        "PRINT_FORMAT": "{address}"
    }
    for key, value in env_vars.items():
        monkeypatch.setenv(key, value)

@pytest.fixture
def mock_streetview_image():
    """Create a mock Street View image response"""
    return BytesIO(b"fake-image-data")

@pytest.fixture
def mock_dependencies(mock_streetview_image):
    """Mock all external dependencies"""
    patches = {
        'everylot.bot.BlueskyPoster': Mock(),
        'everylot.bot.TwitterPoster': Mock(),
        'everylot.everylot.requests.get': Mock()
    }
    
    # Configure mock responses
    patches['everylot.bot.BlueskyPoster'].return_value.post.return_value = "bsky_post_uri"
    patches['everylot.bot.TwitterPoster'].return_value.post.return_value = "12345"
    patches['everylot.everylot.requests.get'].return_value.content = mock_streetview_image.getvalue()
    
    with patch.multiple('', **patches):
        yield patches

class TestBot:
    def test_main_successful_run(self, test_db_path, mock_env, mock_dependencies, caplog):
        """Test successful bot execution with both platforms enabled"""
        caplog.set_level(logging.INFO)
        
        # Run bot with test database
        with patch('sys.argv', ['bot.py', '--database', test_db_path]):
            main()
        
        # Verify Bluesky post
        mock_dependencies['everylot.bot.BlueskyPoster'].return_value.post.assert_called_once_with(
            "123 Main St",
            ANY  # Street View image
        )
        
        # Verify Twitter post
        mock_dependencies['everylot.bot.TwitterPoster'].return_value.post.assert_called_once_with(
            "123 Main St",
            ANY,  # Street View image
            lat=41.8781,
            lon=-87.6298
        )
        
        # Verify database update
        conn = sqlite3.connect(test_db_path)
        c = conn.cursor()
        c.execute("SELECT posted_twitter, posted_bluesky FROM lots WHERE id = '1407115016'")
        posted_values = c.fetchone()
        conn.close()
        
        assert posted_values[0] == "12345"  # Twitter post ID
        assert posted_values[1] == "bsky_post_uri"  # Bluesky post URI
        assert "Posted to Bluesky" in caplog.text
        assert "Posted to Twitter" in caplog.text

    def test_main_bluesky_only(self, test_db_path, mock_env, mock_dependencies, monkeypatch):
        """Test bot execution with only Bluesky enabled"""
        monkeypatch.setenv("ENABLE_TWITTER", "false")
        
        with patch('sys.argv', ['bot.py', '--database', test_db_path]):
            main()
        
        # Verify only Bluesky was called
        mock_dependencies['everylot.bot.BlueskyPoster'].return_value.post.assert_called_once()
        mock_dependencies['everylot.bot.TwitterPoster'].return_value.post.assert_not_called()

    def test_main_twitter_only(self, test_db_path, mock_env, mock_dependencies, monkeypatch):
        """Test bot execution with only Twitter enabled"""
        monkeypatch.setenv("ENABLE_BLUESKY", "false")
        monkeypatch.setenv("ENABLE_TWITTER", "true")
        
        with patch('sys.argv', ['bot.py', '--database', test_db_path]):
            main()
        
        # Verify only Twitter was called
        mock_dependencies['everylot.bot.BlueskyPoster'].return_value.post.assert_not_called()
        mock_dependencies['everylot.bot.TwitterPoster'].return_value.post.assert_called_once()

    def test_main_specific_pin10(self, test_db_path, mock_env, mock_dependencies):
        """Test bot execution starting from specific PIN10"""
        with patch('sys.argv', ['bot.py', '--database', test_db_path, '--id', '1407115017']):
            main()
        
        # Verify correct lot was posted
        mock_dependencies['everylot.bot.BlueskyPoster'].return_value.post.assert_called_once_with(
            "125 Main St",
            ANY
        )

    def test_main_dry_run(self, test_db_path, mock_env, mock_dependencies):
        """Test dry run mode"""
        with patch('sys.argv', ['bot.py', '--database', test_db_path, '--dry-run']):
            main()
        
        # Verify no posts were made
        mock_dependencies['everylot.bot.BlueskyPoster'].return_value.post.assert_not_called()
        mock_dependencies['everylot.bot.TwitterPoster'].return_value.post.assert_not_called()
        
        # Verify database wasn't updated
        conn = sqlite3.connect(test_db_path)
        c = conn.cursor()
        c.execute("SELECT posted_twitter, posted_bluesky FROM lots WHERE id = '1407115016'")
        posted_values = c.fetchone()
        conn.close()
        
        assert posted_values[0] == '0'  # Twitter not posted
        assert posted_values[1] == '0'  # Bluesky not posted

    def test_main_no_lots_found(self, test_db_path, mock_env, mock_dependencies, caplog):
        """Test handling when no lots are found"""
        # Empty the database
        conn = sqlite3.connect(test_db_path)
        c = conn.cursor()
        c.execute("DELETE FROM lots")
        conn.commit()
        conn.close()
        
        caplog.set_level(logging.ERROR)
        
        with patch('sys.argv', ['bot.py', '--database', test_db_path]):
            main()
        
        assert "No lot found" in caplog.text
        mock_dependencies['everylot.bot.BlueskyPoster'].return_value.post.assert_not_called()
        mock_dependencies['everylot.bot.TwitterPoster'].return_value.post.assert_not_called()

    def test_main_posting_error(self, test_db_path, mock_env, mock_dependencies, caplog):
        """Test handling of posting errors"""
        caplog.set_level(logging.ERROR)
        
        # Make Bluesky post fail
        mock_dependencies['everylot.bot.BlueskyPoster'].return_value.post.side_effect = Exception("Bluesky error")
        
        with patch('sys.argv', ['bot.py', '--database', test_db_path]):
            main()
        
        assert "Failed to post to Bluesky" in caplog.text
        # Twitter should still have posted
        mock_dependencies['everylot.bot.TwitterPoster'].return_value.post.assert_called_once()
