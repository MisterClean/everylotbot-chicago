#!/usr/bin/env python3
import sqlite3
import logging
from io import BytesIO
import requests
import os

NEXT_LOT_QUERY = """
    SELECT *
    FROM lots
    WHERE id > ?
    AND posted_{} = '0'
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

        # Set address formats - default to just address since city/state are constant
        self.search_format = search_format or os.getenv('SEARCH_FORMAT', '{address}, CHICAGO, IL')
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
            # Determine which platform we're posting to
            platform = 'bluesky' if os.getenv('ENABLE_BLUESKY', 'true').lower() == 'true' else 'twitter'
            
            # Check if we have any posted lots
            cursor = self.conn.execute(f"""
                SELECT id FROM lots 
                WHERE posted_{platform} != '0'
                ORDER BY id DESC
                LIMIT 1
            """)
            row = cursor.fetchone()
            
            # Get the last posted lot's ID
            if row:
                start_id = row['id']
            else:
                # If no lots have been posted yet, check if START_PIN10 lot is already posted
                start_id = os.getenv('START_PIN10', '0')
                if start_id != '0':
                    cursor = self.conn.execute(f"""
                        SELECT posted_{platform} FROM lots 
                        WHERE id = ?
                    """, (start_id,))
                    row = cursor.fetchone()
                    # If START_PIN10 lot is already posted, use it as start_id to find next
                    # If not posted, get that specific lot
                    if row and row[0] != '0':
                        start_id = start_id  # Use as starting point for next lot
                    else:
                        cursor = self.conn.execute(SPECIFIC_LOT_QUERY, (start_id,))
                        row = cursor.fetchone()
                        if row:
                            self.lot = dict(row)
                            return
            
            # Get the next unposted lot after start_id
            cursor = self.conn.execute(NEXT_LOT_QUERY.format(platform), (start_id,))

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

        fov, _ = self.aim_camera()  # Get FOV but use configured pitch
        params.update({
            'fov': fov,
            'pitch': float(os.getenv('STREETVIEW_PITCH', -10)),
            'zoom': float(os.getenv('STREETVIEW_ZOOM', 0.8))
        })

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
        Uses the formatted address with hardcoded city/state since this is Chicago-specific.
        Only falls back to lat/lon if address formatting completely fails.
        
        Args:
            key (str): Google Geocoding API key
            
        Returns:
            str: Location string for Street View API
        """
        try:
            # Get the address and ensure it's not empty/None
            address = self.lot.get('address')
            if not address:
                raise ValueError('No address available')
                
            # Format with hardcoded city/state since this is Chicago-specific
            location = f"{address}, CHICAGO, IL"
            self.logger.debug('Using formatted address for Street View: %s', location)
            return location
            
        except (KeyError, ValueError) as e:
            # Only use lat/lon if we have valid coordinates
            lat = self.lot.get('lat', 0.0)
            lon = self.lot.get('lon', 0.0)
            if lat == 0.0 and lon == 0.0:
                raise ValueError(f"No valid location data available: {str(e)}")
            
            self.logger.warning('Could not use address (%s), using lat/lon: %f,%f', str(e), lat, lon)
            return f"{lat},{lon}"

    def sanitize_address(self, address):
        """
        Convert address components into a clean, readable format.
        Example: '2023 N DAMEN AVE' -> '2023 North Damen Avenue'
        
        Args:
            address (str): Raw address string
            
        Returns:
            str: Sanitized address string
        """
        if not address:
            return address

        # Split address into components
        parts = address.strip().split(',')[0].split()  # Take first part before comma
        if not parts:
            return address

        # Direction mapping
        directions = {
            'N': 'North',
            'S': 'South',
            'E': 'East',
            'W': 'West'
        }

        # Street type mapping
        street_types = {
            'AVE': 'Avenue',
            'ST': 'Street',
            'BLVD': 'Boulevard',
            'RD': 'Road',
            'DR': 'Drive',
            'CT': 'Court',
            'PL': 'Place',
            'TER': 'Terrace',
            'LN': 'Lane',
            'WAY': 'Way',
            'CIR': 'Circle',
            'PKY': 'Parkway',
            'SQ': 'Square'
        }

        # Process each part
        result = []
        for i, part in enumerate(parts):
            part = part.strip()
            if i == 0:  # Street number
                result.append(part)
            elif part in directions:  # Direction
                result.append(directions[part])
            elif part in street_types:  # Street type
                result.append(street_types[part])
                break  # Stop processing after street type
            else:  # Street name
                result.append(part.capitalize())

        return ' '.join(result)

    def compose(self, media_id_string=None):
        """
        Compose a social media post with location info.
        
        Args:
            media_id_string (str, optional): Media ID for Twitter
            
        Returns:
            dict: Post parameters including status text and location
        """
        # Get the sanitized address
        sanitized_address = self.sanitize_address(self.lot.get('address', ''))
        
        # Create post data with sanitized address
        post_data = dict(self.lot)
        post_data['address'] = sanitized_address
        
        # Format the status text using sanitized address
        status = self.print_format.format(**post_data)
        
        # Build the final post data
        result = {
            "status": status,
            "lat": self.lot.get('lat', 0.0),
            "long": self.lot.get('lon', 0.0),
        }
        
        if media_id_string:
            result["media_ids"] = [media_id_string]
            
        return result

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
