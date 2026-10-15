CREATE PROCEDURE dbo.ChargerClients @mode int
AS
BEGIN
    SET NOCOUNT ON;
    INSERT INTO dbo.Synthese (id) SELECT id FROM stg.Delta WHERE @mode = 1;
    EXEC dbo.Journaliser;
END
GO
