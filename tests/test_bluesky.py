import pytest
from unittest.mock import Mock, patch
import os
from io import BytesIO
from everylot.bluesky import BlueskyPoster

@pytest.fixture
def mock_client():
    """Create a mock atproto client"""
    with patch('everylot.bluesky.Client') as mock_client:
        instance = mock_client.return_value
        instance.login = Mock()
        instance.com.atproto.repo.upload_blob = Mock(return_value={"blob": "fake-blob-ref"})
        instance.com.atproto.repo.create_record = Mock(return_value={"uri": "fake-post-uri"})
        yield instance

@pytest.fixture
def mock_env(monkeypatch):
    """Set up mock environment variables"""
    monkeypatch.setenv("BLUESKY_IDENTIFIER", "test.bsky.social")
    monkeypatch.setenv("BLUESKY_PASSWORD", "test_password")

@pytest.fixture
def sample_image():
    """Create a sample image for testing"""
    return BytesIO(b"fake image data")

class TestBlueskyPoster:
    def test_initialization_success(self, mock_client, mock_env):
        """Test successful initialization of BlueskyPoster"""
        poster = BlueskyPoster()
        
        mock_client.return_value.login.assert_called_once_with(
            "test.bsky.social",
            "test_password"
        )

    def test_initialization_missing_credentials(self, monkeypatch):
        """Test initialization with missing credentials"""
        monkeypatch.delenv("BLUESKY_IDENTIFIER", raising=False)
        monkeypatch.delenv("BLUESKY_PASSWORD", raising=False)
        
        with pytest.raises(ValueError, match="Missing Bluesky credentials"):
            BlueskyPoster()

    def test_post_text_only(self, mock_client, mock_env):
        """Test posting text-only content"""
        poster = BlueskyPoster()
        status_text = "Test post"
        
        post_uri = poster.post(status_text)
        
        assert post_uri == "fake-post-uri"
        mock_client.return_value.com.atproto.repo.create_record.assert_called_once()
        create_args = mock_client.return_value.com.atproto.repo.create_record.call_args[1]
        assert create_args["record"]["text"] == "Test post"
        assert "embed" not in create_args["record"]

    def test_post_with_image(self, mock_client, mock_env, sample_image):
        """Test posting with an image"""
        poster = BlueskyPoster()
        status_text = "Test post with image"
        
        post_uri = poster.post(status_text, sample_image)
        
        # Verify image upload
        mock_client.return_value.com.atproto.repo.upload_blob.assert_called_once()
        upload_args = mock_client.return_value.com.atproto.repo.upload_blob.call_args
        assert upload_args[0][0] == sample_image  # First positional arg should be image data
        assert upload_args[0][1] == "image/jpeg"  # Second positional arg should be content type
        
        # Verify post creation with image
        create_args = mock_client.return_value.com.atproto.repo.create_record.call_args[1]
        assert create_args["record"]["text"] == "Test post with image"
        assert create_args["record"]["embed"]["$type"] == "app.bsky.embed.images"
        assert create_args["record"]["embed"]["images"][0]["image"]["blob"] == "fake-blob-ref"

    def test_login_failure(self, mock_client, mock_env):
        """Test handling of login failure"""
        mock_client.return_value.login.side_effect = Exception("Login failed")
        
        with pytest.raises(Exception, match="Login failed"):
            BlueskyPoster()

    def test_post_failure(self, mock_client, mock_env):
        """Test handling of post creation failure"""
        poster = BlueskyPoster()
        mock_client.return_value.com.atproto.repo.create_record.side_effect = Exception("Post failed")
        
        with pytest.raises(Exception, match="Post failed"):
            poster.post("Test post")

    def test_image_upload_failure(self, mock_client, mock_env, sample_image):
        """Test handling of image upload failure"""
        poster = BlueskyPoster()
        mock_client.return_value.com.atproto.repo.upload_blob.side_effect = Exception("Upload failed")
        
        with pytest.raises(Exception, match="Upload failed"):
            poster.post("Test post", sample_image)

    def test_custom_logger(self, mock_client, mock_env):
        """Test using a custom logger"""
        mock_logger = Mock()
        poster = BlueskyPoster(logger=mock_logger)
        
        # Should log successful login
        mock_logger.debug.assert_called_with("Successfully logged into Bluesky")
        
        # Test logging during post
        poster.post("Test post")
        mock_logger.debug.assert_called_with("Successfully posted to Bluesky: fake-post-uri")
