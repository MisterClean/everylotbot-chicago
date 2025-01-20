import pytest
from unittest.mock import Mock, patch
import os
from io import BytesIO
from everylot.twitter import TwitterPoster

@pytest.fixture
def mock_tweepy_api():
    """Create a mock tweepy API"""
    with patch('everylot.twitter.tweepy') as mock_tweepy:
        # Mock the auth handler
        mock_auth = Mock()
        mock_tweepy.OAuth1UserHandler.return_value = mock_auth
        
        # Mock the API
        mock_api = Mock()
        mock_tweepy.API.return_value = mock_api
        
        # Mock successful media upload
        mock_media = Mock()
        mock_media.media_id_string = "fake_media_id"
        mock_api.media_upload.return_value = mock_media
        
        # Mock successful status update
        mock_status = Mock()
        mock_status.id = 12345
        mock_api.update_status.return_value = mock_status
        
        yield mock_tweepy

@pytest.fixture
def mock_env(monkeypatch):
    """Set up mock environment variables"""
    monkeypatch.setenv("TWITTER_CONSUMER_KEY", "test_consumer_key")
    monkeypatch.setenv("TWITTER_CONSUMER_SECRET", "test_consumer_secret")
    monkeypatch.setenv("TWITTER_ACCESS_TOKEN", "test_access_token")
    monkeypatch.setenv("TWITTER_ACCESS_TOKEN_SECRET", "test_access_token_secret")

@pytest.fixture
def sample_image():
    """Create a sample image for testing"""
    return BytesIO(b"fake image data")

class TestTwitterPoster:
    def test_initialization_success(self, mock_tweepy_api, mock_env):
        """Test successful initialization of TwitterPoster"""
        poster = TwitterPoster()
        
        # Verify OAuth setup
        mock_tweepy_api.OAuth1UserHandler.assert_called_once_with(
            "test_consumer_key",
            "test_consumer_secret",
            "test_access_token",
            "test_access_token_secret"
        )
        
        # Verify API creation
        mock_tweepy_api.API.assert_called_once()

    def test_initialization_missing_credentials(self, monkeypatch):
        """Test initialization with missing credentials"""
        monkeypatch.delenv("TWITTER_CONSUMER_KEY", raising=False)
        monkeypatch.delenv("TWITTER_CONSUMER_SECRET", raising=False)
        monkeypatch.delenv("TWITTER_ACCESS_TOKEN", raising=False)
        monkeypatch.delenv("TWITTER_ACCESS_TOKEN_SECRET", raising=False)
        
        with pytest.raises(ValueError, match="Missing Twitter credentials"):
            TwitterPoster()

    def test_post_text_only(self, mock_tweepy_api, mock_env):
        """Test posting text-only content"""
        poster = TwitterPoster()
        status_text = "Test tweet"
        
        tweet_id = poster.post(status_text)
        
        assert tweet_id == "12345"
        mock_tweepy_api.API.return_value.update_status.assert_called_once_with(
            status=status_text,
            media_ids=None,
            lat=None,
            long=None
        )

    def test_post_with_image(self, mock_tweepy_api, mock_env, sample_image):
        """Test posting with an image"""
        poster = TwitterPoster()
        status_text = "Test tweet with image"
        
        tweet_id = poster.post(status_text, sample_image)
        
        # Verify media upload
        mock_tweepy_api.API.return_value.media_upload.assert_called_once_with(
            'image.jpg',
            file=sample_image
        )
        
        # Verify status update with media
        mock_tweepy_api.API.return_value.update_status.assert_called_once_with(
            status=status_text,
            media_ids=["fake_media_id"],
            lat=None,
            long=None
        )
        
        assert tweet_id == "12345"

    def test_post_with_location(self, mock_tweepy_api, mock_env):
        """Test posting with location data"""
        poster = TwitterPoster()
        status_text = "Test tweet with location"
        lat, lon = 41.8781, -87.6298  # Chicago coordinates
        
        tweet_id = poster.post(status_text, lat=lat, lon=lon)
        
        mock_tweepy_api.API.return_value.update_status.assert_called_once_with(
            status=status_text,
            media_ids=None,
            lat=lat,
            long=lon
        )

    def test_auth_failure(self, mock_tweepy_api, mock_env):
        """Test handling of authentication failure"""
        mock_tweepy_api.OAuth1UserHandler.side_effect = Exception("Auth failed")
        
        with pytest.raises(Exception, match="Auth failed"):
            TwitterPoster()

    def test_media_upload_failure(self, mock_tweepy_api, mock_env, sample_image):
        """Test handling of media upload failure"""
        mock_tweepy_api.API.return_value.media_upload.side_effect = Exception("Upload failed")
        poster = TwitterPoster()
        
        with pytest.raises(Exception, match="Failed to post to Twitter"):
            poster.post("Test tweet", sample_image)

    def test_status_update_failure(self, mock_tweepy_api, mock_env):
        """Test handling of status update failure"""
        mock_tweepy_api.API.return_value.update_status.side_effect = Exception("Update failed")
        poster = TwitterPoster()
        
        with pytest.raises(Exception, match="Failed to post to Twitter"):
            poster.post("Test tweet")

    def test_custom_logger(self, mock_tweepy_api, mock_env):
        """Test using a custom logger"""
        mock_logger = Mock()
        poster = TwitterPoster(logger=mock_logger)
        
        # Should log successful authentication
        mock_logger.debug.assert_called_with("Successfully authenticated with Twitter")
        
        # Test logging during post
        poster.post("Test tweet")
        mock_logger.debug.assert_called_with("Successfully posted to Twitter: 12345")
