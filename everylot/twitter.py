#!/usr/bin/env python3
import os
import logging
import tweepy

class TwitterPoster:
    def __init__(self, logger=None):
        """Initialize the Twitter poster with credentials from environment."""
        self.logger = logger or logging.getLogger('everylot.twitter')
        
        # Get credentials from environment
        self.consumer_key = os.getenv("TWITTER_CONSUMER_KEY")
        self.consumer_secret = os.getenv("TWITTER_CONSUMER_SECRET")
        self.access_token = os.getenv("TWITTER_ACCESS_TOKEN")
        self.access_token_secret = os.getenv("TWITTER_ACCESS_TOKEN_SECRET")
        
        if not all([self.consumer_key, self.consumer_secret, 
                   self.access_token, self.access_token_secret]):
            raise ValueError("Missing Twitter credentials in environment")
        
        self.api = self._get_api()

    def _get_api(self):
        """Create and return an authenticated Twitter API object."""
        try:
            auth = tweepy.OAuth1UserHandler(
                self.consumer_key,
                self.consumer_secret,
                self.access_token,
                self.access_token_secret
            )
            api = tweepy.API(auth)
            self.logger.debug("Successfully authenticated with Twitter")
            return api
        except Exception as e:
            self.logger.error(f"Failed to authenticate with Twitter: {str(e)}")
            raise

    def post(self, status_text, image_data=None, lat=None, lon=None):
        """
        Post to Twitter with optional image and location.
        
        Args:
            status_text (str): The text content to post
            image_data (bytes-like object): Optional image data to upload
            lat (float): Optional latitude for the post
            lon (float): Optional longitude for the post
            
        Returns:
            str: The ID of the created tweet
        """
        try:
            media_ids = []
            if image_data:
                # Upload the image
                media = self.api.media_upload('image.jpg', file=image_data)
                media_ids.append(media.media_id_string)
                self.logger.debug(f"Successfully uploaded media: {media.media_id_string}")

            # Create the tweet
            tweet = self.api.update_status(
                status=status_text,
                media_ids=media_ids if media_ids else None,
                lat=lat,
                long=lon
            )
            
            self.logger.debug(f"Successfully posted to Twitter: {tweet.id}")
            return str(tweet.id)

        except Exception as e:
            self.logger.error(f"Failed to post to Twitter: {str(e)}")
            raise
