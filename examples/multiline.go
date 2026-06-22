package example

const someNameQuery = `SELECT id, name FROM users WHERE enabled = TRUE`

const modelByDeploymentParamsQuery = `
	SELECT
	    :test,
	    u.id,
	    u.project,
	    u.created_at,
	    u.updated_at,
	    r.resource_name
	FROM
	    users u
	JOIN resources r ON u.resource_id = r.id
	WHERE
	    u.name = $1
	    AND r.name = $2
`

const updateSystemModelQuery = `
	UPDATE
	users
	SET
	    name = :name,
	    updated_at = :updated_at,
	    updated_by = :updated_by
	WHERE
	    id = :id
`

const updateSomethingQuery = `UPDATE something SET value = :value WHERE id = $1`
