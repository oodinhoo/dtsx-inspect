CREATE PROCEDURE dbo.Charger
AS
BEGIN
    SET NOCOUNT ON;
    INSERT INTO dbo.Clients (id, courriel) SELECT s.id, LOWER(s.email) FROM stg.Clients s;
    EXEC dbo.Archiver;
END
GO
