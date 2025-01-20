#!/usr/bin/env python3
from datetime import datetime
from atproto import Client
import os
import logging

class BlueskyPoster:
    def __init__(self, logger=None):
        """Initialize the Bluesky poster with credentials from environment."""
        self.logger = logger or logging.getLogger('everylot.bluesky')
        self.identifier = os.getenv("BLUESKY_IDENTIFIER")
        self.password = os.getenv("BLUESKY_PASSWORD")
        
        if not all([self.identifier, self.password]):
            raise ValueError("Missing Bluesky credentials in environment")
        
        self.client = Client()
        self._login()

    def _login(self):
        """Login to Bluesky."""
        try:
            self.client.login(self.identifier, self.password)
            self.logger.debug("Successfully logged into Bluesky")
        except Exception as e:
            self.logger.error(f"Failed to login to Bluesky: {str(e)}")
            raise

    def post(self, status_text, image_data=None, pin10=None, clean_address=None):
        """
        Post to Bluesky with optional image.
        
        Args:
            status_text (str): The text content to post
            image_data (bytes-like object): Optional image data to upload
            pin10 (str): The PIN10 identifier for the property
            clean_address (str): The sanitized address of the property
            
        Returns:
            str: The URI of the created post
        """
        try:
            record = {
                "collection": "app.bsky.feed.post",
                "repo": self.identifier,
                "record": {
                    "text": status_text,
                    "createdAt": datetime.utcnow().isoformat() + "Z",
                }
            }

            if image_data:
                # Upload the image blob
                upload_resp = self.client.com.atproto.repo.upload_blob(image_data)
                
                # Add image to the post
                record["record"]["embed"] = {
                    "$type": "app.bsky.embed.images",
                    "images": [{
                        "image": upload_resp["blob"],
                        "alt": f"Google Streetview of the property with PIN10 {pin10}: {clean_address}"
                    }]
                }

            # Create the post
            resp = self.client.com.atproto.repo.create_record(data=record)
            uri = resp["uri"]
            # Convert AT Protocol URI to web URL
            # at://did:plc:xxx/app.bsky.feed.post/zzz -> https://bsky.app/profile/did:plc:xxx/post/zzz
            try:
                did = uri.split("/")[2]
                post_id = uri.split("/")[-1]
                web_url = f"https://bsky.app/profile/{did}/post/{post_id}"
                self.logger.debug(f"Successfully posted to Bluesky: {web_url}")
                return web_url
            except Exception as e:
                self.logger.warning(f"Failed to convert URI to web URL: {str(e)}")
                return uri  # Fall back to original URI if conversion fails

        except Exception as e:
            self.logger.error(f"Failed to post to Bluesky: {str(e)}")
            raise
