CREATE PROCEDURE dbo.Journaliser
AS
    INSERT INTO dbo.Journal (message) VALUES ('chargement');
GO
