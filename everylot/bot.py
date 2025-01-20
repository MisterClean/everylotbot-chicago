#!/usr/bin/env python3
import argparse
import logging
import os
from dotenv import load_dotenv
from . import __version__ as version
from .everylot import EveryLot
from .bluesky import BlueskyPoster
from .twitter import TwitterPoster

def main():
    load_dotenv()

    parser = argparse.ArgumentParser(description='every lot bot for Twitter and Bluesky')
    parser.add_argument('--database', type=str, default=os.getenv('DATABASE_PATH', 'cook_county_lots.db'),
                      help='path to SQLite lots database')
    parser.add_argument('--id', type=str, default=os.getenv('START_PIN10'),
                      help='start with this PIN10 ID')
    parser.add_argument('-s', '--search-format', type=str, 
                      default=os.getenv('SEARCH_FORMAT', '{address}, {city} {state}'),
                      help='Python format string for searching Google')
    parser.add_argument('-p', '--print-format', type=str,
                      default=os.getenv('PRINT_FORMAT', '{address}'),
                      help='Python format string for post text')
    parser.add_argument('--dry-run', action='store_true',
                      help='Do not actually post')
    parser.add_argument('-v', '--verbose', action='store_true',
                      help='Show debug output')
    args = parser.parse_args()

    # Setup logging
    log_level = logging.DEBUG if args.verbose else logging.INFO
    logging.basicConfig(level=log_level)
    logger = logging.getLogger('everylot')

    # Initialize the lot finder
    el = EveryLot(args.database,
                  logger=logger,
                  print_format=args.print_format,
                  search_format=args.search_format,
                  id_=args.id)

    if not el.lot:
        logger.error('No lot found')
        return

    logger.debug('%s address: %s', el.lot['id'], el.lot.get('address'))
    logger.debug('db location %s,%s', el.lot['lat'], el.lot['lon'])

    # Get the streetview image
    google_key = os.getenv('GOOGLE_API_KEY')
    image = el.get_streetview_image(google_key)

    # Initialize posters based on environment settings
    post_ids = []
    enable_bluesky = os.getenv('ENABLE_BLUESKY', 'true').lower() == 'true'
    enable_twitter = os.getenv('ENABLE_TWITTER', 'false').lower() == 'true'

    if not (enable_bluesky or enable_twitter):
        logger.error('Neither Bluesky nor Twitter is enabled')
        return

    # Compose the post data with sanitized address
    post_data = el.compose()
    logger.info(f"Post text: {post_data['status']}")

    if not args.dry_run:
        if enable_bluesky:
            try:
                bluesky = BlueskyPoster(logger=logger)
                # Get clean address for ALT text
                clean_address = el.sanitize_address(el.lot['address'])
                post_id = bluesky.post(post_data['status'], image, pin10=el.lot['id'], clean_address=clean_address)
                el.mark_as_posted('bluesky', post_id)
                logger.info("Posted to Bluesky")
            except Exception as e:
                logger.error(f"Failed to post to Bluesky: {e}")

        if enable_twitter:
            try:
                twitter = TwitterPoster(logger=logger)
                post_id = twitter.post(
                    post_data['status'], 
                    image,
                    lat=post_data['lat'],
                    lon=post_data['long']
                )
                el.mark_as_posted('twitter', post_id)
                logger.info("Posted to Twitter")
            except Exception as e:
                logger.error(f"Failed to post to Twitter: {e}")

if __name__ == '__main__':
    main()
