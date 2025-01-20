#!/usr/bin/env python3
import sqlite3
import logging
from io import BytesIO
import requests
import os

NEXT_LOT_QUERY = """
    SELECT *
    FROM lots
    WHERE id >= ?
    AND (
        (posted_twitter = '0' AND posted_bluesky = '0')
        OR (posted_twitter = '0' AND ? = 'twitter')
        OR (posted_bluesky = '0' AND ? = 'bluesky')
    )
    ORDER BY id ASC
    LIMIT 1;
"""

SPECIFIC_LOT_QUERY = """
    SELECT *
    FROM lots
    WHERE id = ?
    LIMIT 1;
"""

SVAPI = "https://maps.googleapis.com/maps/api/streetview"
GCAPI = "https://maps.googleapis.com/maps/api/geocode/json"

class EveryLot:

    def __init__(self, database, search_format=None, print_format=None, id_=None, **kwargs):
        """
        Initialize EveryLot with database connection and formatting options.
        
        Args:
            database (str): Path to SQLite database file
            search_format (str, optional): Format string for Google Street View search
            print_format (str, optional): Format string for social media posts
            id_ (str, optional): Specific PIN10 ID to use
            **kwargs: Additional options including logger
        """
        self.logger = kwargs.get('logger', logging.getLogger('everylot'))

        # Set address formats
        self.search_format = search_format or os.getenv('SEARCH_FORMAT', '{address}, {city} {state}')
        self.print_format = print_format or os.getenv('PRINT_FORMAT', '{address}')

        self.logger.debug('Search format: %s', self.search_format)
        self.logger.debug('Print format: %s', self.print_format)

        # Connect to database
        self.conn = sqlite3.connect(database)
        self.conn.row_factory = sqlite3.Row

        # Get the next lot
        if id_:
            # Get specific PIN10
            cursor = self.conn.execute(SPECIFIC_LOT_QUERY, (id_,))
        else:
            # Get next untweeted PIN10
            cursor = self.conn.execute(NEXT_LOT_QUERY, ('0',))

        row = cursor.fetchone()
        self.lot = dict(row) if row else None

    def aim_camera(self):
        """Calculate optimal camera settings based on building height."""
        # Default values for a typical 2-story building
        fov, pitch = 65, 10

        try:
            floors = float(self.lot.get('floors', 2))
            if floors == 3:
                fov = 72
            elif floors == 4:
                fov, pitch = 76, 15
            elif floors >= 5:
                fov, pitch = 81, 20
            elif floors == 6:
                fov = 86
            elif floors >= 8:
                fov, pitch = 90, 25
            elif floors >= 10:
                fov, pitch = 90, 30
        except (TypeError, ValueError):
            pass

        return fov, pitch

    def get_streetview_image(self, key):
        """
        Fetch image from Google Street View API.
        
        Args:
            key (str): Google Street View API key
            
        Returns:
            BytesIO: Image data
        """
        if not key:
            raise ValueError("Google Street View API key is required")

        params = {
            "location": self.streetviewable_location(key),
            "key": key,
            "size": "1000x1000"
        }

        params['fov'], params['pitch'] = self.aim_camera()

        try:
            r = requests.get(SVAPI, params=params)
            r.raise_for_status()
            self.logger.debug('Street View URL: %s', r.url)

            sv = BytesIO()
            for chunk in r.iter_content(chunk_size=8192):
                sv.write(chunk)

            sv.seek(0)
            return sv

        except requests.exceptions.RequestException as e:
            self.logger.error('Failed to fetch Street View image: %s', str(e))
            raise

    def streetviewable_location(self, key):
        """
        Determine the best location for Street View image.
        Checks if Google-geocoded address is nearby, otherwise uses lat/lon.
        
        Args:
            key (str): Google Geocoding API key
            
        Returns:
            str: Location string for Street View API
        """
        # Try to format address from lot data
        try:
            address = self.search_format.format(**self.lot)
        except KeyError:
            self.logger.warning('Could not format address, using lat/lon')
            return f"{self.lot['lat']},{self.lot['lon']}"

        # Check if we have coordinates to validate against
        try:
            d = 0.007  # ~0.5 mile radius
            bounds = {
                'min_lat': self.lot['lat'] - d,
                'max_lat': self.lot['lat'] + d,
                'min_lon': self.lot['lon'] - d,
                'max_lon': self.lot['lon'] + d
            }
        except KeyError:
            self.logger.info('No coordinates for validation. Using address directly.')
            return address

        # Geocode the address
        try:
            r = requests.get(GCAPI, params={"address": address, "key": key})
            r.raise_for_status()
            
            result = r.json()
            if not result.get('results'):
                raise ValueError('No geocoding results found')

            loc = result['results'][0]['geometry']['location']

            # Check if geocoded location is within bounds
            if (bounds['min_lon'] <= loc['lng'] <= bounds['max_lon'] and
                bounds['min_lat'] <= loc['lat'] <= bounds['max_lat']):
                self.logger.debug('Using formatted address for Street View')
                return address
            else:
                raise ValueError('Geocoded location outside expected bounds')

        except Exception as e:
            self.logger.info('Geocoding failed (%s), using stored coordinates', str(e))
            return f"{self.lot['lat']},{self.lot['lon']}"

    def compose(self, media_id_string=None):
        """
        Compose a social media post with location info.
        
        Args:
            media_id_string (str, optional): Media ID for Twitter
            
        Returns:
            dict: Post parameters including status text and location
        """
        # Format the status text
        status = self.print_format.format(**self.lot)
        
        # Build the post data
        post_data = {
            "status": status,
            "lat": self.lot.get('lat', 0.0),
            "long": self.lot.get('lon', 0.0),
        }
        
        if media_id_string:
            post_data["media_ids"] = [media_id_string]
            
        return post_data

    def mark_as_posted(self, platform, post_id):
        """
        Mark the current lot as posted for a specific platform.
        
        Args:
            platform (str): Platform name ('twitter' or 'bluesky')
            post_id (str): ID of the post
        """
        column = f"posted_{platform.lower()}"
        self.conn.execute(
            f"UPDATE lots SET {column} = ? WHERE id = ?",
            (post_id, self.lot['id'])
        )
        self.conn.commit()
