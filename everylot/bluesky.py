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

    def post(self, status_text, image_data=None):
        """
        Post to Bluesky with optional image.
        
        Args:
            status_text (str): The text content to post
            image_data (bytes-like object): Optional image data to upload
            
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
                upload_resp = self.client.com.atproto.repo.upload_blob(image_data, "image/jpeg")
                
                # Add image to the post
                record["record"]["embed"] = {
                    "$type": "app.bsky.embed.images",
                    "images": [{
                        "image": upload_resp["blob"],
                        "alt": "Property photo"
                    }]
                }

            # Create the post
            resp = self.client.com.atproto.repo.create_record(**record)
            self.logger.debug(f"Successfully posted to Bluesky: {resp['uri']}")
            return resp["uri"]

        except Exception as e:
            self.logger.error(f"Failed to post to Bluesky: {str(e)}")
            raise
