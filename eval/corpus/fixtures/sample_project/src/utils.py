"""Utility functions for the sample project."""


def parse_input(raw: str) -> dict:
    """Parse raw input string into a dictionary."""
    parts = raw.split(",")
    return {k.strip(): v.strip() for k, v in (p.split("=") for p in parts)}


def validate_email(email: str) -> bool:
    """Check if an email address is valid."""
    return "@" in email and "." in email


class DataProcessor:
    """Processes and transforms data records."""

    def __init__(self, config: dict):
        self.config = config
        self.records = []

    def add_record(self, record: dict):
        """Add a record to the processing queue."""
        self.records.append(record)

    def process_all(self) -> list:
        """Process all queued records."""
        return [self._transform(r) for r in self.records]

    def _transform(self, record: dict) -> dict:
        return {k: str(v).upper() for k, v in record.items()}
