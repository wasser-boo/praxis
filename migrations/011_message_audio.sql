-- Reply audio stays with its message, including across dashboard reconnects.
-- History queries read only the MIME metadata; bytes are served on demand.
ALTER TABLE messages ADD COLUMN audio BLOB;
ALTER TABLE messages ADD COLUMN audio_mime TEXT;
