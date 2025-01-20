# EveryLot Chicago Bot

A bot that posts images of every property lot in Chicago to Bluesky and/or Twitter. This is a modernized version of the original everylotbot, specifically configured for Chicago property data.

## Features

- Fetches property data from Cook County Data Portal
- Supports posting to both Bluesky and Twitter
- Uses Google Street View for property images
- Maintains a local SQLite database of properties
- Can start from a specific PIN10 property ID
- Modern Python 3.10+ implementation

## Architecture

```
┌─────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│  Cook County    │     │   Google APIs    │     │  Social Media    │
│   Data Portal   │     │                  │     │   Platforms      │
└────────┬────────┘     └────────┬─────────┘     └────────┬─────────┘
         │                       │                         │
         ▼                       ▼                         │
┌─────────────────┐     ┌──────────────────┐             │
│  data_ingest.py │     │   everylot.py    │             │
│                 │     │                   │             │
│ - Fetch data    │     │ - Image fetching  │             │
│ - Sort by PIN14 │     │ - Address lookup  │             │
│ - Deduplicate   │     │ - Camera angles   │             │
└────────┬────────┘     └────────┬─────────┘             │
         │                       │                         │
         ▼                       ▼                         │
┌──────────────────────────────────────────┐             │
│              SQLite Database             │             │
│                                         │             │
│ - Property records                      │             │
│ - Platform-specific posting status      │             │
│   (Twitter and Bluesky)                 │             │
└────────────────────┬───────────────────┘             │
                     │                                  │
                     ▼                                  ▼
              ┌─────────────────┐            ┌──────────────────┐
              │     bot.py      │            │  Social Modules  │
              │                 │            │                  │
              │ - Main logic    ├───────────►│ - bluesky.py    │
              │ - Coordination  │            │ - twitter.py     │
              └─────────────────┘            └──────────────────┘

Configuration (.env):
├── API Credentials
├── Platform Settings
├── Formatting Options
└── Runtime Configuration

Data Flow:
1. data_ingest.py fetches and processes property data
2. Data is stored in SQLite database
3. bot.py coordinates image fetching and posting
4. everylot.py handles Street View interaction
5. Social modules manage platform-specific posting
```

## Setup

1. Clone the repository:
```bash
git clone https://github.com/yourusername/everylotbot-chicago.git
cd everylotbot-chicago
```

2. Install dependencies:
```bash
pip install -r requirements.txt
```

3. Create a `.env` file from the example:
```bash
cp .env.example .env
```

4. Configure your `.env` file with the required credentials:
- Cook County Data Portal token
- Google Street View API key
- Bluesky credentials
- Twitter credentials (optional)
- Other settings as needed

## Initial Data Import

Before running the bot, you need to fetch and process the Cook County property data:

```bash
python data_ingest.py --year 2023 --city CHICAGO
```

This will:
1. Fetch property data from Cook County
2. Sort by PIN14 ascending
3. De-duplicate by PIN10
4. Create a local SQLite database (`cook_county_lots.db`)

## Running the Bot

Basic usage:
```bash
python -m everylot.bot
```

Options:
- `--database`: Path to SQLite database (default: from .env or cook_county_lots.db)
- `--id`: Start with a specific PIN10
- `-s/--search-format`: Format string for Google Street View searches
- `-p/--print-format`: Format string for post text
- `--dry-run`: Test without posting
- `-v/--verbose`: Show debug output

## Configuration

The bot can be configured through environment variables in your `.env` file:

```bash
# Required tokens
CHICAGO_DATA_PORTAL_TOKEN=your_token_here
GOOGLE_API_KEY=your_google_api_key

# Bluesky credentials
BLUESKY_IDENTIFIER=your.bsky.social
BLUESKY_PASSWORD=your_password

# Twitter credentials (optional)
TWITTER_CONSUMER_KEY=your_key
TWITTER_CONSUMER_SECRET=your_secret
TWITTER_ACCESS_TOKEN=your_token
TWITTER_ACCESS_TOKEN_SECRET=your_token_secret

# Toggles
ENABLE_TWITTER=false
ENABLE_BLUESKY=true

# Optional settings
START_PIN10=           # Start from this PIN10 (see "Starting Point Behavior" below)
SEARCH_FORMAT="{address}, {city} {state}"
PRINT_FORMAT="{address}"
DATABASE_PATH=cook_county_lots.db

# Starting Point Behavior
When START_PIN10 is set:
1. During initial data import (data_ingest.py):
   - All PINs up to and including START_PIN10 are marked as posted ('1')
   - This effectively skips these properties when the bot runs
2. During bot operation:
   - If START_PIN10 is not yet posted, bot starts with that PIN
   - If START_PIN10 is already posted, bot starts with the next unposted PIN
3. After successful posts:
   - The posted_bluesky column stores the web URL of the post
   - Format: https://bsky.app/profile/[handle]/post/[id]

# Camera settings
STREETVIEW_PITCH=-11.55  # Camera angle (default: -10)
STREETVIEW_ZOOM=1        # Zoom level (default: 0.8)
```

## Running Automatically

To run the bot automatically, set up a cron job. For example, to post every hour:

```bash
0 * * * * cd /path/to/everylotbot-chicago && python -m everylot.bot >> bot.log 2>&1
```

## Development

### Project Structure
```
everylotbot-chicago/
├── data_ingest.py      # Data fetching and processing
├── requirements.txt    # Python dependencies
├── .env.example       # Environment variables template
├── pytest.ini         # Test configuration
├── tests/             # Test suite
│   ├── conftest.py    # Shared test fixtures
│   ├── test_bot.py    # Bot integration tests
│   ├── test_data_ingest.py  # Data ingestion tests
│   ├── test_bluesky.py      # Bluesky module tests
│   ├── test_twitter.py      # Twitter module tests
│   └── test_everylot.py     # Core functionality tests
└── everylot/          # Main bot package
    ├── __init__.py
    ├── bot.py         # Main bot logic
    ├── everylot.py    # Core functionality
    ├── bluesky.py     # Bluesky posting
    └── twitter.py     # Twitter posting
```

### Testing

The project includes a comprehensive test suite using pytest. To run the tests:

```bash
# Install test dependencies
pip install -r requirements.txt

# Run all tests with coverage report
pytest

# Run specific test file
pytest tests/test_bot.py

# Run tests with detailed output
pytest -v

# Run tests and show print statements
pytest -s
```

Test coverage reports will show which lines of code are covered by tests.

### Database Schema

The SQLite database (`cook_county_lots.db`) contains a single table `lots` with the following schema:

- `id` (TEXT): Primary key, the PIN10 identifier
- `address` (TEXT): Full property address
- `lat` (REAL): Latitude coordinate
- `lon` (REAL): Longitude coordinate
- `posted_twitter` (TEXT): Twitter post ID or '0' if not posted
- `posted_bluesky` (TEXT): Either:
  - '0': Not posted
  - '1': Marked as posted (for pins before START_PIN10)
  - URL: Web link to the Bluesky post (e.g., https://bsky.app/profile/handle/post/id)

### Image ALT Text Format

The bot uses a standardized format for image ALT text to ensure accessibility and consistent property identification:

```
Google Streetview of the property with PIN10 [PIN10]: [clean address]
```

For example:
```
Google Streetview of the property with PIN10 1234567890: 2023 North Damen Avenue
```

The address is automatically cleaned and formatted (e.g., "N" → "North", "AVE" → "Avenue") for better readability.

### Development Guidelines

1. **Code Style**
   - Follow PEP 8 guidelines
   - Use type hints where appropriate
   - Include docstrings for all functions and classes

2. **Testing**
   - Write tests for new features
   - Maintain test coverage above 80%
   - Use fixtures from conftest.py when possible
   - Mock external dependencies (APIs, databases)

3. **Environment**
   - Use .env for configuration
   - Never commit sensitive credentials
   - Document new environment variables in .env.example

4. **Git Workflow**
   - Create feature branches for new work
   - Write clear commit messages
   - Update tests and documentation
   - Run full test suite before committing

### Adding New Features

1. **New Social Platform**
   - Create new module in everylot/
   - Follow TwitterPoster/BlueskyPoster pattern
   - Add configuration to .env.example
   - Create corresponding test file
   - Update bot.py to handle new platform

2. **Data Source Changes**
   - Update data_ingest.py
   - Maintain PIN10/PIN14 logic
   - Update database schema if needed
   - Add new environment variables
   - Update corresponding tests

3. **Configuration Changes**
   - Document in .env.example
   - Update README.md
   - Add validation in code
   - Update tests to cover new options

## Credits

This is a modernized fork of [everylotbot](https://github.com/fitnr/everylotbot) by Neil Freeman, updated for Chicago property data and modern social media platforms.

## License

This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
