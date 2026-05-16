#!/usr/bin/env bash
# apply-supabase-migrations.sh
# Apply Supabase database migrations for JD structured extraction pipeline
#
# Prerequisites:
#   1. Supabase CLI installed (npm install -g supabase)
#   2. Logged in: supabase login
#   3. Linked: supabase link --project-ref mivspjqggjiypupwsgqr
#
# Usage:
#   ./scripts/apply-supabase-migrations.sh
#
# The migrations directory is at supabase/migrations/ and contains:
#   20260502203201_add_jd_structured_columns.sql
#   20260502203202_create_jd_structured_indexes.sql
#   20260502203203_create_extraction_runs_table.sql
#   20260502203204_add_dead_letter_columns.sql

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "Applying Supabase migrations from $PROJECT_DIR/supabase/migrations/"
echo ""

# Check if supabase CLI is available
if ! command -v supabase &>/dev/null; then
  echo "ERROR: supabase CLI not found."
  echo "  Install: npm install -g supabase"
  echo "  Or use the Management API alternative below."
  echo ""
  echo "Alternative: Apply via Supabase Management API:"
  echo "  ACCESS_TOKEN=your_sbp_token"
  echo "  for f in supabase/migrations/*.sql; do"
  echo "    SQL=\$(cat \"\$f\")"
  echo "    curl -s -X POST \"https://api.supabase.com/v1/projects/mivspjqggjiypupwsgqr/sql\" \\"
  echo "      -H \"Authorization: Bearer \$ACCESS_TOKEN\" \\"
  echo "      -H \"Content-Type: application/json\" \\"
  echo "      -d \"{\\\"query\\\": \\\"\$SQL\\\"}\""
  echo "  done"
  exit 1
fi

cd "$PROJECT_DIR"

# Check if project is linked
if ! supabase status 2>/dev/null | grep -q "Project URL"; then
  echo "Linking to Supabase project mivspjqggjiypupwsgqr..."
  supabase link --project-ref mivspjqggjiypupwsgqr
fi

# Apply migrations
echo "Pushing migrations to Supabase..."
supabase db push

echo ""
echo "Migrations applied successfully."

echo ""
echo "Verification: checking tables and columns..."
echo "  Run: supabase db diff"
echo "  Or connect via psql: psql \"postgresql://postgres:YOUR_PASSWORD@db.mivspjqggjiypupwsgqr.supabase.co:5432/postgres\""
