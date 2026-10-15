CREATE PROCEDURE [dbo].[Charger] @mode int
AS
BEGIN
    SET NOCOUNT ON;
    INSERT INTO dbo.Clients (id) SELECT id FROM stg.Clients;
    EXEC dbo.Journaliser;
END
GO
