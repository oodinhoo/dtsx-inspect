CREATE PROCEDURE dbo.Copier
AS
    INSERT INTO dbo.Copie (id) SELECT id FROM dbo.Source;
GO
